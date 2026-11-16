"""Real stdio MCP + FFmpeg continuity test in a disposable database.
Color footage is an explicit fixture, never a generated-series quality claim.
"""
import hashlib,json,os,select,sqlite3,subprocess,tempfile,time
from pathlib import Path
repo=Path(__file__).resolve().parents[1]
binary=Path(os.environ.get('DRAMAFACTORY_BINARY',repo/'target/release/dramafactory'))
local=Path.home()/'Library/Application Support/studio.DramaFactory.DramaFactory/studio.sqlite'
with sqlite3.connect(local) as db:
    config=json.loads(db.execute("SELECT value FROM settings WHERE key='worker'").fetchone()[0])
with tempfile.TemporaryDirectory(prefix='df-story-test-') as temporary:
    root=Path(temporary)
    p=subprocess.Popen([str(binary),'--mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=dict(os.environ,DRAMAFACTORY_DATABASE=str(root/'studio.sqlite')))
    seq=0
    def request(method,params):
        global seq
        seq+=1;p.stdin.write(json.dumps(dict(jsonrpc='2.0',id=seq,method=method,params=params))+'\n');p.stdin.flush()
        assert select.select([p.stdout],[],[],20)[0],method
        r=json.loads(p.stdout.readline());assert r['id']==seq,r
        return r['result']
    def call(tool_name,fail=False,**args):
        r=request('tools/call',dict(name='dramafactory_'+tool_name,arguments=args))
        if fail:
            assert r.get('isError'),r
            return r
        assert not r.get('isError'),r
        return json.loads(r['content'][0]['text'])
    request('initialize',dict(protocolVersion='2025-06-18',capabilities={},clientInfo=dict(name='Story continuity integration',version='v002')))
    p.stdin.write(json.dumps(dict(jsonrpc='2.0',method='notifications/initialized'))+'\n');p.stdin.flush()
    db=sqlite3.connect(root/'studio.sqlite')
    db.execute("INSERT INTO settings VALUES('worker',?)",(json.dumps(config),));db.commit()
    series=call('create_series',name='Disposable story test',premise='A clerk discovers a cover-up.')
    character=call('create_character',series_id=series['id'],name='Mara',description='Hotel clerk; recurring identity')
    eps=[call('create_episode',series_id=series['id'],title=title) for title in ['The code','The disclosure']]
    footage=root/'fixture.mp4'
    subprocess.run([config['ffmpeg'],'-v','error','-f','lavfi','-i','color=c=navy:size=128x224:rate=24','-t','2','-c:v','libx264',str(footage)],check=True)
    db.execute("INSERT INTO assets VALUES('fixture','fixture',?,?,128,224,2,?,'approved')",(str(footage),hashlib.sha256(footage.read_bytes()).hexdigest(),json.dumps(dict(kind='synthetic_test_fixture'))));db.commit()
    location=call('create_location',series_id=series['id'],name='Hotel office',description='Records office behind reception')
    camera=call('save_camera_setup',location_id=location['id'],name='A',framing='Medium close-up',placement='Across the desk',eyeline='Camera right',screen_axis='Mara left, owner right')
    shot_ids=[]
    for e in eps:
        shot=call('create_shot',episode_id=e['id'],title='Explicit fixture',prompt='Not generated footage',duration=2.)
        shot_ids.append(shot['id'])
        call('assign_shot_asset',shot_id=shot['id'],asset_id='fixture')
        call('save_screenplay',episode_id=e['id'],text='INT. HOTEL — NIGHT. Mara acts on evidence.')
    ready=call('get_episode_readiness',episode_id=eps[0]['id'])
    assert ready['export_request_valid'] and ready['shot_count']==1 and not any(i['blocks_export'] for i in ready['issues']),ready
    # A valid request alone must not hide a missing on-disk picture.
    moved=footage.with_suffix('.retained.mp4');footage.rename(moved)
    missing=call('get_episode_readiness',episode_id=eps[0]['id'])
    assert not missing['export_request_valid'] and any(i['scope']=='picture' and 'missing' in i['action'] for i in missing['issues']),missing
    moved.rename(footage)
    call('save_shot_direction',shot_id=shot_ids[0],direction=dict(character_ids=[],location_id=location['id'],camera_setup_id=camera['id'],dramatic_beat='Mara learns the truth',performance='Controlled shock',wardrobe='Navy blazer'))
    tone=root/'voice-fixture.wav'
    subprocess.run([config['ffmpeg'],'-v','error','-f','lavfi','-i','sine=frequency=440:duration=1','-c:a','pcm_s16le',str(tone)],check=True)
    db.execute("INSERT INTO assets VALUES('voice-fixture','fixture',?,?,0,0,1,?,'approved')",(str(tone),hashlib.sha256(tone.read_bytes()).hexdigest(),json.dumps(dict(kind='synthetic_test_fixture'))))
    db.execute("INSERT INTO dialogue(id,shot_id,text,voice,speed,asset_id) VALUES('fixture-line',?,'A voiced fixture.','fixture',1,'voice-fixture')",(shot_ids[0],));db.commit()
    plan=call('get_series_plan',series_id=series['id']);assert plan['revision']==0
    plan.update(audience='Adults who enjoy contained mysteries',genre='Hotel mystery',theme='Trust earned through truth',central_question='Will Mara expose the hotel owner?',ending='Mara releases the evidence and refuses the hush money.',characters=[dict(character_id=character['id'],desire='Protect her brother',flaw='Distrusts everyone',transformation='Trusts an ally and speaks publicly')],initial_facts=[dict(key='Mara knows code',value='no')],episodes=[dict(episode_id=eps[0]['id'],goal='Find the safe code',conflict='Owner returns early',turn='Brother wrote the code',payoff='She opens the safe',hook='Inside is her own personnel file',ending_resolution='',requires=[dict(key='Mara knows code',value='no')],changes=[dict(key='Mara knows code',**{'from':'no'},to='yes')]),dict(episode_id=eps[1]['id'],goal='Expose the cover-up',conflict='Owner offers hush money',turn='Her ally has a second copy',payoff='Evidence goes public',hook='',ending_resolution='She rejects payment and publishes proof, resolving whether she will speak.',requires=[dict(key='Mara knows code',value='yes')],changes=[dict(key='Evidence public',**{'from':None},to='yes')])],threads=[dict(setup_episode_id=eps[0]['id'],payoff_episode_id=eps[1]['id'],setup='Brother left a second envelope',payoff='Ally reveals the backup copy')])
    old=json.loads(json.dumps(plan));plan=call('save_series_plan',plan=plan)
    # Outline order must win over creation order, without losing unplanned drafts.
    extra=call('create_episode',series_id=series['id'],title='Unplanned draft')
    reordered=json.loads(json.dumps(plan));reordered['episodes'].reverse()
    reordered=call('save_series_plan',plan=reordered)
    assert [e['id'] for e in call('list_episodes',series_id=series['id'])]==[eps[1]['id'],eps[0]['id'],extra['id']]
    plan['revision']=reordered['revision'];plan=call('save_series_plan',plan=plan)
    assert [e['id'] for e in call('list_episodes',series_id=series['id'])]==[eps[0]['id'],eps[1]['id'],extra['id']]
    call('save_series_plan',fail=True,plan=old)
    report=call('get_continuity',series_id=series['id']);assert not report['issues'] and all(not e['issues'] for e in report['episodes']),report
    assert report['canon']=={'Mara knows code':'no'}
    assert report['episodes'][1]['entry_state']['Mara knows code']=='yes'
    plan['episodes'][1]['requires'][0]['value']='no'
    plan=call('save_series_plan',plan=plan)
    assert call('get_continuity',series_id=series['id'])['episodes'][1]['issues']
    call('commit_episode',fail=True,series_id=series['id'],episode_id=eps[0]['id'],export_id='fixture')
    plan['episodes'][1]['requires'][0]['value']='yes';plan=call('save_series_plan',plan=plan)
    def export(e):
        job=call('export_episode',episode_id=e['id']);end=time.monotonic()+90
        while time.monotonic()<end:
            state=call('get_job_status',job_id=job['id'])
            assert state['status'] not in ['failed','blocked','canceled'],state
            if state['status']=='completed':break
            time.sleep(.2)
        else:raise TimeoutError(job)
        call('review_asset',asset_id=state['output_id'],review='approved')
        return state['output_id']
    finals=[export(e) for e in eps]
    def commit(index,fail=False):return call('commit_episode',fail=fail,series_id=series['id'],episode_id=eps[index]['id'],export_id=finals[index])
    commit(1,True)
    # A cut exported from an earlier screenplay must not be blessed as current.
    call('save_screenplay',episode_id=eps[0]['id'],text='REVISED SCRIPT: Mara confronts the owner.')
    commit(0,True)
    finals[0]=export(eps[0]);report=commit(0)
    assert report['episodes'][0]['status']=='committed' and report['canon']=={'Mara knows code':'yes'},report
    report=commit(1);assert all(e['status']=='committed' for e in report['episodes']),report
    assert report['canon']['Evidence public']=='yes'
    repeated=commit(1)
    assert repeated['episodes'][1]['commit_id']==report['episodes'][1]['commit_id']
    review=call('get_asset_review',asset_id=finals[0]);review['note']='Editorial note only; no media or approval change.'
    call('save_asset_review',review=review)
    assert all(e['status']=='committed' for e in call('get_continuity',series_id=series['id'])['episodes'])
    call('set_dialogue_visibility',line_id='fixture-line',speaker_visible=False,expected_visible=True)
    assert all(e['status']=='stale' for e in call('get_continuity',series_id=series['id'])['episodes'])
    call('set_dialogue_visibility',line_id='fixture-line',speaker_visible=True,expected_visible=False)
    assert all(e['status']=='committed' for e in call('get_continuity',series_id=series['id'])['episodes'])
    fades=dict(fade_in=.25,fade_out=.25);zero=dict(fade_in=0.,fade_out=0.)
    call('set_picture_fades',edit=dict(shot_id=shot_ids[0],fades=fades,expected=zero))
    assert all(e['status']=='stale' for e in call('get_continuity',series_id=series['id'])['episodes'])
    call('set_picture_fades',edit=dict(shot_id=shot_ids[0],fades=zero,expected=fades))
    assert all(e['status']=='committed' for e in call('get_continuity',series_id=series['id'])['episodes'])
    # Changing an in-point invalidates the dependent canon even when total
    # duration and selected source stay the same; restoring it restores canon.
    first_shot=call('list_shots',episode_id=eps[0]['id'])[0]
    # The fixture is 2s long, so a shorter interval is used to make room.
    trim=dict(shot_id=first_shot['id'],source_start=.25,duration=1.5,expected_source_start=0.,expected_duration=2.,expected_asset_id=first_shot['asset_id'])
    call('trim_shot',trim=trim)
    assert all(e['status']=='stale' for e in call('get_continuity',series_id=series['id'])['episodes'])
    # Existing dialogue still fits after restoring the source interval.
    trim.update(source_start=0.,duration=2.,expected_source_start=.25,expected_duration=1.5)
    call('trim_shot',trim=trim)
    assert all(e['status']=='committed' for e in call('get_continuity',series_id=series['id'])['episodes'])
    # Source/script edits invalidate the full dependent chain without deleting cuts.
    call('save_screenplay',episode_id=eps[0]['id'],text='ANOTHER REVISION')
    report=call('get_continuity',series_id=series['id'])
    assert [e['status'] for e in report['episodes']]==['stale','stale'],report
    assert report['canon']=={'Mara knows code':'no'}
    finals[0]=export(eps[0]);report=commit(0)
    assert [e['status'] for e in report['episodes']]==['committed','stale'],report
    report=commit(1);assert all(e['status']=='committed' for e in report['episodes'])
    original=footage.read_bytes();footage.write_bytes(original+b'changed-fixture')
    assert all(e['status']=='stale' for e in call('get_continuity',series_id=series['id'])['episodes'])
    footage.write_bytes(original)
    assert all(e['status']=='committed' for e in call('get_continuity',series_id=series['id'])['episodes'])
    # Editing a brief preserves identity assets but invalidates dependent canon.
    brief=dict(id=character['id'],name=character['name'],description='A revised acting brief',expected_name=character['name'],expected_description=character['description'])
    revised=call('update_library_entry',edit=brief)
    assert all(e['status']=='stale' for e in call('get_continuity',series_id=series['id'])['episodes'])
    call('update_library_entry',edit=dict(id=character['id'],name=character['name'],description=character['description'],expected_name=revised['name'],expected_description=revised['description']))
    assert all(e['status']=='committed' for e in call('get_continuity',series_id=series['id'])['episodes'])
    call('assign_character_voice',character_id=character['id'],voice='am_adam')
    assert all(e['status']=='stale' for e in call('get_continuity',series_id=series['id'])['episodes'])
    call('assign_character_voice',character_id=character['id'],voice='af_heart')
    assert all(e['status']=='committed' for e in call('get_continuity',series_id=series['id'])['episodes'])
    call('set_dialogue_timing',line_id='fixture-line',timeline_start=.25)
    assert all(e['status']=='stale' for e in call('get_continuity',series_id=series['id'])['episodes'])
    call('set_dialogue_timing',line_id='fixture-line',timeline_start=None)
    assert all(e['status']=='committed' for e in call('get_continuity',series_id=series['id'])['episodes'])
    call('set_caption_mode',episode_id=eps[0]['id'],burn_in=True)
    assert all(e['status']=='stale' for e in call('get_continuity',series_id=series['id'])['episodes'])
    call('set_caption_mode',episode_id=eps[0]['id'],burn_in=False)
    assert all(e['status']=='committed' for e in call('get_continuity',series_id=series['id'])['episodes'])
    camera['eyeline']='Camera left'
    camera=call('save_camera_setup',**camera)
    assert all(e['status']=='stale' for e in call('get_continuity',series_id=series['id'])['episodes'])
    finals[0]=export(eps[0]);commit(0);report=commit(1)
    assert all(e['status']=='committed' for e in report['episodes'])
    call('review_asset',asset_id=finals[0],review='rejected')
    assert all(e['status']=='stale' for e in call('get_continuity',series_id=series['id'])['episodes'])
    call('review_asset',asset_id=finals[0],review='approved')
    plan['theme']='The truth survives intimidation';plan=call('save_series_plan',plan=plan)
    assert all(e['status']=='stale' for e in call('get_continuity',series_id=series['id'])['episodes'])
    assert db.execute('SELECT COUNT(*) FROM story_history').fetchone()[0]==6
    assert db.execute('SELECT COUNT(*) FROM canon_commits').fetchone()[0]==6
    assert len(call('list_assets'))>=5
    p.stdin.close();assert p.wait(timeout=10)==0
    db.close()
    print('Story integration passed: revision conflicts/history, cast IDs, two-episode fact propagation, contradiction detection, reviewed export/script matching, sequential canon, dialogue/caption changes, downstream invalidation and retained commits/cuts.')
