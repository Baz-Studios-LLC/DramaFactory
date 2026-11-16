"""Exercise actual stdio MCP against an isolated temporary database."""
import json
import os
import subprocess
import tempfile
import uuid
import sqlite3
from pathlib import Path

with tempfile.TemporaryDirectory(prefix="dramafactory-mcp-") as temporary:
    env = dict(os.environ, DRAMAFACTORY_DATABASE=str(Path(temporary) / "studio.sqlite"))
    binary = Path(os.environ.get("DRAMAFACTORY_BINARY", str(Path(__file__).resolve().parents[1] / "target/release/dramafactory")))
    process = subprocess.Popen([str(binary), "--mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, env=env)
    sequence = 0

    def request(method, params):
        global sequence
        sequence += 1
        process.stdin.write(json.dumps(dict(jsonrpc="2.0", id=sequence, method=method, params=params)) + "\n")
        process.stdin.flush()
        response = json.loads(process.stdout.readline())
        assert response["id"] == sequence
        return response["result"]

    def tool(tool_name, **arguments):
        response = request("tools/call", dict(name="dramafactory_" + tool_name, arguments=arguments))
        assert not response.get("isError", False), response
        return json.loads(response["content"][0]["text"])

    assert request("initialize", dict(protocolVersion="2025-06-18", capabilities={}, clientInfo=dict(name="DramaFactory integration test", version="v002")))["serverInfo"]["name"] == "DramaFactory"
    process.stdin.write(json.dumps(dict(jsonrpc="2.0", method="notifications/initialized"))+"\n")
    process.stdin.flush()
    assert len(request("tools/list", {})["tools"]) >= 24
    models=tool('list_available_models')
    assert len(models)==5 and all(not m['configured'] for m in models),models
    assert all(m['readiness_scope'] for m in models)
    assert len(next(m for m in models if m['id']=='flux2-klein-4b-q4')['missing_files'])==5
    missing_runtime=tool('check_model_runtime',model_id='kokoro-int8')
    assert not missing_runtime['passed'] and missing_runtime['errors'] and missing_runtime['imports'] is None
    assert 'No checkpoint loaded' in missing_runtime['scope']
    invalid_runtime=request('tools/call',dict(name='dramafactory_check_model_runtime',arguments=dict(model_id='unknown')))
    assert invalid_runtime.get('isError'),invalid_runtime

    # Review notes are shared authoring state, not part of media provenance.
    with sqlite3.connect(Path(temporary)/'studio.sqlite') as fixture_db:
        fixture_db.execute("INSERT INTO assets VALUES('review-fixture','fixture','explicit-fixture','fixture-hash',1,1,0,'{}','unreviewed')")
    review=tool('get_asset_review',asset_id='review-fixture');assert review['revision']==0
    stale_review=dict(review);review.update(decision='rejected',note='Speaking begins too late; keep the original take.')
    saved_review=tool('save_asset_review',review=review)
    assert saved_review['revision']==1
    assert tool('save_asset_review',review=saved_review)['revision']==1
    stale_review['note']='A stale note must not overwrite the rejection.'
    failure=request('tools/call',dict(name='dramafactory_save_asset_review',arguments=dict(review=stale_review)))
    assert failure.get('isError'),failure
    tool('review_asset',asset_id='review-fixture',review='approved')
    current_review=tool('get_asset_review',asset_id='review-fixture')
    assert current_review['note']==saved_review['note'] and current_review['revision']==2
    history=tool('get_asset_review_history',asset_id='review-fixture')
    assert [item['decision'] for item in history]==['approved','rejected'] and history[0]['created_at']
    series = tool("create_series", name="Integration test", premise="Temporary test data")
    episode = tool("create_episode", series_id=series["id"], title="Pilot")
    tool("save_screenplay", episode_id=episode["id"], text="INT. STUDIO — NIGHT")
    assert tool("list_episodes", series_id=series["id"])[0]["screenplay"] == "INT. STUDIO — NIGHT"
    tool('save_screenplay',episode_id=episode['id'],text='REVISED',expected_text='INT. STUDIO — NIGHT')
    response=request('tools/call',dict(name='dramafactory_save_screenplay',arguments=dict(episode_id=episode['id'],text='STALE',expected_text='INT. STUDIO — NIGHT')))
    assert response.get('isError'),response
    assert tool('get_episode',episode_id=episode['id'])['screenplay']=='REVISED'
    shot = tool("create_shot", episode_id=episode["id"],title="Opening",prompt="Rain",camera="Wide",duration=5.0)
    assert tool("list_shots", episode_id=episode["id"])[0]["id"] == shot["id"]
    # Catalog filters intersect, search is literal, and page order follows insertion.
    with sqlite3.connect(Path(temporary)/'studio.sqlite') as fixture_db:
        for asset_id, width, duration, decision in [('catalog-video',704,3.,'rejected'),('catalog-audio',0,2.,'unreviewed')]:
            metadata=json.dumps({'request':{'shot_id':shot['id'],'prompt':"100% actor's coverage"}})
            fixture_db.execute("INSERT INTO assets VALUES(?,?,?,?,?,?,?,?,?)",(asset_id,'fixture','explicit-fixture','fixture-hash',width,width,duration,metadata,decision))
    page=tool('find_assets',limit=1)
    assert page['total']==3 and page['assets'][0]['id']=='catalog-audio',page
    second=tool('find_assets',limit=1,offset=1)
    assert second['total']==3 and second['assets'][0]['id']=='catalog-video'
    assert tool('find_assets',limit=1,offset=3)['assets']==[]
    assert [a['id'] for a in tool('find_assets',text="100% actor's",media='video',review='rejected')['assets']]==['catalog-video']
    assert tool('find_assets',text='100_',media='video')['total']==0
    assert tool('find_assets',text='Integration test',media='audio')['total']==1
    assert tool('find_assets',text='Opening',media='video')['total']==1
    assert tool('find_assets',text='speaking begins',media='image',review='approved')['total']==1
    assert tool('find_assets',media='image',review='rejected')['total']==0
    assert tool('find_assets',media='audio')['assets'][0]['id']=='catalog-audio'
    changed_review=tool('get_asset_review',asset_id='review-fixture')
    changed_review['note']='Revisited mouth detail'
    tool('save_asset_review',review=changed_review)
    assert tool('find_assets',text='mouth detail')['assets'][0]['id']=='review-fixture'
    assert tool('find_assets',text='speaking begins')['total']==0
    assert tool('find_assets',limit=1)['assets'][0]['id']=='catalog-audio'
    for invalid in [dict(limit=0),dict(limit=101),dict(media='movie'),dict(review='accepted'),dict(text='x'*501),dict(offset=-1),dict(unknown=True)]:
        failure=request('tools/call',dict(name='dramafactory_find_assets',arguments=invalid))
        assert failure.get('isError'),failure
    # Read-only checklist enumerates all missing coverage, not just the first gate.
    check_episode=tool('create_episode',series_id=series['id'],title='Checklist fixture')
    check_shots=[tool('create_shot',episode_id=check_episode['id'],title=title,prompt='Fixture',camera='Close',duration=2.) for title in ['First','Reverse']]
    before_jobs=tool('list_jobs')
    checklist=tool('get_episode_readiness',episode_id=check_episode['id'])
    assert checklist['shot_count']==2 and not checklist['export_request_valid']
    assert {i['entity_id'] for i in checklist['issues'] if i['scope']=='picture'}=={s['id'] for s in check_shots}
    assert tool('list_jobs')==before_jobs
    draft=dict(id=str(uuid.uuid4()),shot_id=shot['id'],text='These are my own words.',voice='af_heart',speed=1.,timeline_start=None,revision=0)
    saved=tool('save_dialogue_draft',line=draft)
    assert saved['revision']==1 and saved['asset_id'] is None
    checklist=tool('get_episode_readiness',episode_id=episode['id'])
    assert checklist['dialogue_count']==1 and any(i['scope']=='dialogue' and i['entity_id']==saved['id'] for i in checklist['issues'])
    saved.pop('asset_id')
    assert tool('save_dialogue_draft',line=saved)['revision']==1
    stale=dict(saved);saved['text']='And I can change them before installing a model.'
    saved=tool('save_dialogue_draft',line=saved)
    assert saved['revision']==2
    response=request('tools/call',dict(name='dramafactory_save_dialogue_draft',arguments=dict(line=stale)))
    assert response.get('isError') and 'another session' in response['content'][0]['text'],response
    assert tool('list_dialogue',shot_id=shot['id'])[0]['text']==saved['text']
    performance=request('tools/call',dict(name='dramafactory_generate_performance',arguments=dict(shot_id=shot['id'],audio_asset_id='missing')))
    assert performance.get('isError') and 'Configure' in performance['content'][0]['text'],performance
    saved.pop('asset_id')
    tool('set_dialogue_timing',line_id=saved['id'],timeline_start=.5)
    response=request('tools/call',dict(name='dramafactory_save_dialogue_draft',arguments=dict(line=saved)))
    assert response.get('isError'),response
    assert tool('list_dialogue',shot_id=shot['id'])[0]['timeline_start']==.5
    jobs_before=tool('list_jobs')
    response=request('tools/call',dict(name='dramafactory_produce_episode_dialogue',arguments=dict(episode_id=episode['id'])))
    assert response.get('isError') and 'not configured' in response['content'][0]['text'],response
    assert tool('list_jobs')==jobs_before
    assert tool('list_dialogue',shot_id=shot['id'])[0]['text']==saved['text']
    tool("create_character",series_id=series["id"],name="Lead",description="A secret identity")
    assert len(tool("list_characters",series_id=series["id"])) == 1
    entry=tool('list_characters',series_id=series['id'])[0]
    brief=dict(id=entry['id'],name='Lead revised',description='An honest revised brief',expected_name=entry['name'],expected_description=entry['description'])
    tool('assign_character_voice',character_id=entry['id'],voice='am_michael')
    revised=tool('update_library_entry',edit=brief)
    assert revised['id']==entry['id'] and revised['reference']==entry['reference'] and revised['voice']=='am_michael'
    assert tool('update_library_entry',edit=brief)==revised
    stale=dict(brief,name='An obsolete edit')
    failure=request('tools/call',dict(name='dramafactory_update_library_entry',arguments=dict(edit=stale)))
    assert failure.get('isError') and 'another session' in failure['content'][0]['text'],failure
    assert tool('list_characters',series_id=series['id'])[0]==revised
    # Queue snapshots are computed by the service, not trusted from callers.
    reference=Path(temporary)/'reference-fixture.dat';reference.write_bytes(b'explicit conditioning fixture, never sent to inference')
    image_job=tool('generate_image',prompt='Explicit snapshot fixture',reference=str(reference),library_id=entry['id'])
    assert image_job['status']=='blocked'
    with sqlite3.connect(Path(temporary)/'studio.sqlite') as fixture_db:
        image_request=json.loads(fixture_db.execute('SELECT request FROM jobs WHERE id=?',(image_job['id'],)).fetchone()[0])
    import hashlib
    assert image_request['reference_hash']==hashlib.sha256(reference.read_bytes()).hexdigest()
    assert image_request['library_context_before']==dict(name=revised['name'],description=revised['description'],reference_hash=None)


    location=tool("create_location",series_id=series["id"],name="Studio",description="Window left, door right")
    camera=tool("save_camera_setup",location_id=location["id"],name="A",framing="Wide",placement="Across the room",eyeline="Toward the door",screen_axis="Window left, door right")
    assert tool("list_camera_setups",location_id=location["id"])[0]==camera
    direction=tool("save_shot_direction",shot_id=shot["id"],direction=dict(character_ids=[],location_id=location["id"],camera_setup_id=camera["id"],dramatic_beat="Someone arrives",performance="Natural small movements",wardrobe="Preserve props"))
    assert tool("get_shot_direction",shot_id=shot["id"])==direction
    prepared=tool("prepare_shot_render",shot_id=shot["id"])
    assert prepared["request"]["frames"]==121
    assert "Window left, door right" in prepared["request"]["prompt"]
    assert prepared["notes"] # five-second hold is beyond default 1–4s cut guide
    queued=tool("render_shot",shot_id=shot["id"])
    assert queued["job"]["status"]=="blocked" # no models configured in test DB
    tool("cancel_job",job_id=queued["job"]["id"])
    job = tool("queue_render", prompt="A five-second cinematic shot")
    assert job["status"] == "blocked"
    tool("cancel_job", job_id=job["id"])
    assert tool("list_jobs")[0]["status"] == "canceled"
    current_line=tool('list_dialogue',shot_id=shot['id'])[0]
    assert tool('get_dialogue_direction',line_id=current_line['id'])['speaker_visible'] is True
    tool('set_dialogue_visibility',line_id=current_line['id'],speaker_visible=False,expected_visible=True)
    changed_line=tool('list_dialogue',shot_id=shot['id'])[0]
    assert changed_line['asset_id']==current_line['asset_id'] and changed_line['revision']==current_line['revision']+1
    tool('set_dialogue_visibility',line_id=current_line['id'],speaker_visible=False,expected_visible=True)
    assert tool('list_dialogue',shot_id=shot['id'])[0]['revision']==changed_line['revision']
    conflict=request('tools/call',dict(name='dramafactory_set_dialogue_visibility',arguments=dict(line_id=current_line['id'],speaker_visible=True,expected_visible=True)))
    assert conflict.get('isError'),conflict
    # Hosted cast creation and chat mutation exercise shared services, without AI calls.
    new_series=tool("create_series",name="Hosted cast isolation",premise="Fixture")
    profile_episode=tool("create_episode",series_id=new_series["id"],title="Profile fixture")
    profile_plan=tool("get_generation_plan",episode_id=profile_episode["id"])
    profile_plan["segments"][0]["duration"]=24
    profile_plan["segments"][0]["resolution"]="480p"
    tool("save_generation_plan",plan=profile_plan)
    restored=tool("get_generation_plan",episode_id=profile_episode["id"])
    assert restored["segments"][0]["duration"]==24
    assert restored["segments"][0]["resolution"]=="480p"
    character_id=str(uuid.uuid4())
    args=dict(series_id=new_series["id"],character_id=character_id,name="Tamsin fixture",description="Adult village trader",portrait_prompt="Cinematic village portrait")
    tool("create_character_with_prompt",**args)
    tool("create_character_with_prompt",**args)
    assert tool("get_portrait_style",library_id=character_id)["style"]=="photo"
    tool("save_portrait_style",library_id=character_id,style="animation")
    assert tool("get_portrait_style",library_id=character_id)["style"]=="animation"
    prompt=tool("get_portrait_prompt",library_id=character_id)
    assert prompt["prompt"]==args["portrait_prompt"]
    foreign=tool("create_series",name="Other fixture",premise="")
    result=request("tools/call",dict(name="dramafactory_create_character_with_prompt",arguments={**args,"series_id":foreign["id"]}))
    assert result.get("isError"),result
    message=dict(id=str(uuid.uuid4()),role="user",text="Original text",context="Characters")
    with sqlite3.connect(env["DRAMAFACTORY_DATABASE"]) as connection:
        connection.execute("INSERT INTO sidekick_messages(id,series_id,role,text,context) VALUES(?,?,?,?,?)",(message["id"],new_series["id"],message["role"],message["text"],message["context"]))
    tool("change_user_message",series_id=new_series["id"],message=message,replacement="Edited text")
    stale=request("tools/call",dict(name="dramafactory_change_user_message",arguments=dict(series_id=new_series["id"],message=message,replacement=None)))
    assert stale.get("isError"),stale
    message["text"]="Edited text"
    assert tool("get_sidekick_messages",series_id=new_series["id"])[0]["text"]==message["text"]
    tool("change_user_message",series_id=new_series["id"],message=message,replacement=None)
    assert not tool("get_sidekick_messages",series_id=new_series["id"])
    process.stdin.close()
    assert process.wait(timeout=10) == 0
    print("MCP integration passed: initialize, discovery, series, episode, screenplay, render request, cancellation, saved camera geometry, linked shot direction, prepared render and explicit pacing notes.")
