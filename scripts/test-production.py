"""Real FFmpeg + Kokoro integration in a disposable database.
Synthetic color clips are explicit test fixtures, not AI generation benchmarks.
"""
import hashlib
import json
import os
import select
import sqlite3
import subprocess
import tempfile
import time
from pathlib import Path

repository = Path(__file__).resolve().parents[1]
binary = repository/"target/release/dramafactory"
local_root = Path.home()/"Library/Application Support/studio.DramaFactory.DramaFactory"
source = sqlite3.connect(local_root/"studio.sqlite")
config = json.loads(source.execute("SELECT value FROM settings WHERE key='worker'").fetchone()[0])
source.close()
config["speech_model"] = str(local_root/"models/kokoro")

with tempfile.TemporaryDirectory(prefix="df-production-test-") as temporary:
    root = Path(temporary)
    env = dict(os.environ, DRAMAFACTORY_DATABASE=str(root/"studio.sqlite"))
    p = subprocess.Popen([str(binary), "--mcp"],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=env)
    sequence = 0
    def request(method, params):
        global sequence
        sequence += 1
        p.stdin.write(json.dumps(dict(jsonrpc="2.0",id=sequence,method=method,params=params))+"\n")
        p.stdin.flush()
        if not select.select([p.stdout],[],[],20)[0]:
            raise TimeoutError("MCP response timed out")
        response=json.loads(p.stdout.readline())
        assert response["id"]==sequence, response
        return response["result"]
    def tool(tool_name,**arguments):
        result=request("tools/call",dict(name="dramafactory_"+tool_name,arguments=arguments))
        assert not result.get("isError",False),result
        return json.loads(result["content"][0]["text"])
    request("initialize",dict(protocolVersion="2025-06-18",capabilities={},clientInfo=dict(name="Production test",version="v002")))
    p.stdin.write(json.dumps(dict(jsonrpc="2.0",method="notifications/initialized"))+"\n");p.stdin.flush()
    db=sqlite3.connect(root/"studio.sqlite")
    db.execute("INSERT INTO settings VALUES('worker',?)",(json.dumps(config),));db.commit()
    series=tool("create_series",name="Disposable media integration")
    episode=tool("create_episode",series_id=series["id"],title="Fixture assembly")
    shot=tool("create_shot",episode_id=episode["id"],title="Synthetic color fixture",prompt="Explicit test fixture",duration=4.)
    clip=root/"fixture.mp4"
    subprocess.run([config["ffmpeg"],"-v","error","-f","lavfi","-i","testsrc2=size=128x224:rate=24","-t","4","-c:v","libx264",str(clip)],check=True)
    digest=hashlib.sha256(clip.read_bytes()).hexdigest()
    db.execute("INSERT INTO assets VALUES('fixture','fixture-job',?,?,128,224,4,?,'approved')",(str(clip),digest,json.dumps(dict(kind="synthetic_test_fixture"))))
    db.execute("UPDATE shots SET asset_id='fixture',status='rendered' WHERE id=?",(shot["id"],));db.commit()
    speech=tool("generate_dialogue",shot_id=shot["id"],text="Every great drama begins with a secret.",voice="af_heart",speed=1.)
    def wait(job,timeout=90):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            status=tool("get_job_status",job_id=job["id"])
            if status["status"] in ["failed","canceled","blocked"]:
                raise AssertionError(status)
            if status["status"]=="completed":
                assert status["attempts"]==1,status
                return status
            time.sleep(.2)
        raise TimeoutError("Production job timed out")
    wait(speech)
    lines=tool("list_dialogue",shot_id=shot["id"])
    assert lines[0]["asset_id"]
    export=wait(tool("export_episode",episode_id=episode["id"]))
    assets=tool("list_assets")
    result=next(a for a in assets if a["id"]==export["output_id"])
    assert result["width"]==1080 and result["height"]==1920,result
    assert abs(result["duration"]-4.)<.1,result
    assert result["metadata"]["info"]["audio"],result
    assert result["metadata"]["timeline"]["clips"][0]["audio"],result
    subprocess.run([config["ffmpeg"],"-v","error","-xerror","-i",result["path"],"-f","null","-"],check=True)
    # A separate silent episode measures actual music timing/gain/fades without
    # speech masking the result. Fixtures are synthetic, never benchmark claims.
    sound_episode=tool("create_episode",series_id=series["id"],title="Sound mix fixture")
    sound_shot=tool("create_shot",episode_id=sound_episode["id"],title="Silent fixture",prompt="Explicit test fixture",duration=4.)
    tool("assign_shot_asset",shot_id=sound_shot["id"],asset_id="fixture")
    wav=root/"tone.wav"
    subprocess.run([config["ffmpeg"],"-v","error","-f","lavfi","-i","sine=frequency=440:sample_rate=48000:duration=3","-c:a","pcm_s16le",str(wav)],check=True)
    imported=tool("import_audio",path=str(wav))
    assert imported["path"] != str(wav)
    wav.unlink() # managed copy must survive removal of the external source
    cue_args=dict(episode_id=sound_episode["id"],asset_id=imported["id"],name="Measured tone",kind="music",start=1.,source_start=.5,duration=2.,gain_db=-6.,fade_in=.25,fade_out=.25)
    cue=tool("save_audio_cue",**cue_args)
    assert tool("list_audio_cues",episode_id=sound_episode["id"])[0]==cue
    rejected=request("tools/call",dict(name="dramafactory_export_episode",arguments=dict(episode_id=sound_episode["id"])))
    assert rejected.get("isError"),rejected
    tool("review_asset",asset_id=imported["id"],review="approved")
    bad=dict(cue_args,start=3.)
    changed_cue=tool("save_audio_cue",id=cue["id"],expected=cue,**bad)
    # Another session still holds the earlier cue. Neither its save nor delete
    # may overwrite/remove the newer state. Repeating the accepted save is safe.
    assert tool("save_audio_cue",id=cue["id"],expected=cue,**bad)==changed_cue
    for tool_name,arguments in [('save_audio_cue',dict(id=cue['id'],expected=cue,**cue_args)),('save_audio_cue',dict(id=cue['id'],**cue_args)),('remove_audio_cue',dict(id=cue['id'],expected=cue))]:
        conflict=request('tools/call',dict(name='dramafactory_'+tool_name,arguments=arguments))
        assert conflict.get('isError'),conflict
        assert tool('list_audio_cues',episode_id=sound_episode['id'])==[changed_cue]

    rejected=request("tools/call",dict(name="dramafactory_export_episode",arguments=dict(episode_id=sound_episode["id"])))
    assert rejected.get("isError"),rejected
    tool("save_audio_cue",id=cue["id"],expected=changed_cue,**cue_args)
    mixed=wait(tool("export_episode",episode_id=sound_episode["id"]))
    mix=next(a for a in tool("list_assets") if a["id"]==mixed["output_id"])
    import array,math
    pcm=subprocess.check_output([config["ffmpeg"],"-v","error","-i",mix["path"],"-vn","-ac","1","-ar","48000","-f","f32le","-"])
    samples=array.array("f");samples.frombytes(pcm)
    def rms(start,end):
        chunk=samples[int(start*48000):int(end*48000)]
        return math.sqrt(sum(v*v for v in chunk)/len(chunk))
    steady=rms(1.5,2.5)
    assert .035<steady<.055,steady # -6dB from a 0.125-amplitude sine
    assert rms(.2,.8)<.001 and rms(3.2,3.8)<.001
    assert rms(1.01,1.05)<steady*.3 and rms(2.95,2.99)<steady*.3
    assert mix["metadata"]["timeline"]["cues"][0]["cue"]["source_start"]==.5
    tool("remove_audio_cue",id=cue["id"],expected=cue)
    tool("remove_audio_cue",id=cue["id"],expected=cue) # retry is idempotent
    conflict=request('tools/call',dict(name='dramafactory_save_audio_cue',arguments=dict(id=cue['id'],expected=cue,**cue_args)))
    assert conflict.get('isError'),conflict

    assert not tool("list_audio_cues",episode_id=sound_episode["id"])
    assert Path(imported["path"]).is_file()
    print("Sound mix passed: managed WAV, approval/bounds gates, cue conflict-safe saves/removal and retries, deleted-draft guard, measured onset/offset, -6dB gain and fades in actual AAC export.")
    # Dialogue must remain continuous over a 2s cut, not be clipped or doubled.
    bridge_episode=tool("create_episode",series_id=series["id"],title="Dialogue bridge fixture")
    solid=root/"solid.mp4"
    subprocess.run([config["ffmpeg"],"-v","error","-f","lavfi","-i","color=c=navy:size=128x224:rate=24","-t","2","-c:v","libx264",str(solid)],check=True)
    db.execute("INSERT INTO assets VALUES('solid','fixture',?,?,128,224,2,?,'approved')",(str(solid),hashlib.sha256(solid.read_bytes()).hexdigest(),json.dumps(dict(kind="synthetic_test_fixture"))));db.commit()
    bridge_shots=[]
    for title in ["Speaking coverage","Reaction cut"]:
        item=tool("create_shot",episode_id=bridge_episode["id"],title=title,prompt="Explicit solid-color fixture",duration=2.)
        tool("assign_shot_asset",shot_id=item["id"],asset_id="solid");bridge_shots.append(item)
    wait(tool("generate_dialogue",shot_id=bridge_shots[0]["id"],text="Every great drama begins with a secret.",voice="af_heart",speed=1.))
    line=tool("list_dialogue",shot_id=bridge_shots[0]["id"])[0]
    speech_asset=line["asset_id"]
    assert line["timeline_start"] is None
    rejected=request("tools/call",dict(name="dramafactory_export_episode",arguments=dict(episode_id=bridge_episode["id"])))
    assert rejected.get("isError"),rejected # speech does not fit inside its original shot
    tool("set_dialogue_timing",line_id=line["id"],timeline_start=.4)
    moved=tool("list_dialogue",shot_id=bridge_shots[0]["id"])[0]
    assert moved["asset_id"]==speech_asset and moved["timeline_start"]==.4
    tool('set_dialogue_visibility',line_id=line['id'],speaker_visible=False,expected_visible=True)
    assert tool('list_dialogue',shot_id=bridge_shots[0]['id'])[0]['asset_id']==speech_asset
    timing=tool('get_dialogue_timing',episode_id=bridge_episode['id'])
    assert timing['unmeasured_lines']==0 and not timing['overlaps'] and timing['lines'][0]['start']==.4
    assert timing['gaps_without_dialogue'][0]==dict(start=0.,duration=.4)
    assert timing['lines'][0]['crosses_shot_boundary'] and not timing['lines'][0]['speaker_visible']
    db.execute("INSERT INTO dialogue(id,shot_id,text,voice,speed,asset_id,timeline_start) VALUES('overlap-fixture',?,'Explicit interruption fixture','af_heart',1,?,.6)",(bridge_shots[1]['id'],speech_asset));db.commit()
    timing=tool('get_dialogue_timing',episode_id=bridge_episode['id'])
    assert len(timing['overlaps'])==1 and timing['overlaps'][0]['duration']>1.,timing
    db.execute("DELETE FROM dialogue WHERE id='overlap-fixture'");db.commit()
    tool("set_caption_mode",episode_id=bridge_episode["id"],burn_in=True)
    ready=wait(tool("export_episode",episode_id=bridge_episode["id"]))
    captioned=next(a for a in tool("list_assets") if a["id"]==ready["output_id"])
    assert captioned["metadata"]["captions_burned"] is True
    assert captioned['metadata']['timeline']['clips'][0]['offscreen_dialogue_ids']==[line['id']]
    assert len(captioned["metadata"]["timeline"]["cues"])==1
    assert all(not c["audio"] for c in captioned["metadata"]["timeline"]["clips"])
    subtitle=Path(captioned["metadata"]["subtitle_file"])
    assert "00:00:00,400" in subtitle.read_text() and "Every great drama" in subtitle.read_text()
    assert hashlib.sha256(subtitle.read_bytes()).hexdigest()==captioned["metadata"]["subtitle_sha256"]
    pcm=subprocess.check_output([config["ffmpeg"],"-v","error","-i",captioned["path"],"-vn","-ac","1","-ar","48000","-f","f32le","-"])
    samples=array.array("f");samples.frombytes(pcm)
    assert rms(.05,.25)<.001 and rms(2.1,2.6)>.005 and rms(3.3,3.8)<.001
    def white_caption_pixels(path,when):
        pixels=subprocess.check_output([config["ffmpeg"],"-v","error","-ss",str(when),"-i",path,"-frames:v","1","-vf","scale=270:480","-f","rawvideo","-pix_fmt","rgb24","-"])
        assert len(pixels)==270*480*3
        return sum(all(pixels[(y*270+x)*3+c]>200 for c in range(3)) for y in range(350,465) for x in range(15,255))
    assert white_caption_pixels(captioned["path"],.1)==0
    assert white_caption_pixels(captioned["path"],1.2)>50
    assert white_caption_pixels(captioned["path"],2.1)>50 # actual captions survive the cut
    tool("set_caption_mode",episode_id=bridge_episode["id"],burn_in=False)
    plain_job=wait(tool("export_episode",episode_id=bridge_episode["id"]))
    plain=next(a for a in tool("list_assets") if a["id"]==plain_job["output_id"])
    assert plain["metadata"]["captions_burned"] is False
    assert Path(plain["metadata"]["subtitle_file"]).is_file()
    assert white_caption_pixels(plain["path"],1.2)==0
    tool("set_dialogue_timing",line_id=line["id"],timeline_start=3.5)
    rejected=request("tools/call",dict(name="dramafactory_export_episode",arguments=dict(episode_id=bridge_episode["id"])))
    assert rejected.get("isError"),rejected
    tool("set_dialogue_timing",line_id=line["id"],timeline_start=None)
    assert tool("list_dialogue",shot_id=bridge_shots[0]["id"])[0]["asset_id"]==speech_asset
    print("Dialogue/captions passed: measured speech across a 2s cut, preserved voice, bounds gates, exact SRT/hash, visible burned captions and separate-file mode in real MP4 output.")
    # Audio-first rehearsal must work with no selected video takes, preserving
    # exact dialogue placement across cuts and silent tail in a real PCM WAV.
    tool('set_dialogue_timing',line_id=line['id'],timeline_start=.4)
    db.execute('UPDATE shots SET asset_id=NULL WHERE episode_id=?',(bridge_episode['id'],));db.commit()
    rehearsal=wait(tool('export_dialogue_rehearsal',episode_id=bridge_episode['id']))
    audio=next(a for a in tool('list_assets') if a['id']==rehearsal['output_id'])
    assert audio['metadata']['kind']=='dialogue_rehearsal' and audio['width']==0
    assert abs(audio['duration']-4)<.001
    import wave
    with wave.open(audio['path'],'rb') as wav_reader:
        assert wav_reader.getframerate()==48000 and wav_reader.getnchannels()==2
        values=array.array('h',wav_reader.readframes(wav_reader.getnframes()))
    def rehearsal_rms(start,end):
        chunk=values[int(start*96000):int(end*96000)]
        return math.sqrt(sum((v/32768)**2 for v in chunk)/len(chunk))
    assert rehearsal_rms(.05,.3)<.0001
    assert rehearsal_rms(.6,1.5)>.01
    assert rehearsal_rms(2.05,2.2)>.003
    assert rehearsal_rms(3.8,3.95)<.0001
    assert audio['metadata']['request']['lines'][0]['start']==.4
    # Rejected/missing speech is an authoring error, never silently skipped.
    tool('review_asset',asset_id=speech_asset,review='rejected')
    failure=request('tools/call',dict(name='dramafactory_export_dialogue_rehearsal',arguments=dict(episode_id=bridge_episode['id'])))
    assert failure.get('isError'),failure
    tool('review_asset',asset_id=speech_asset,review='approved')
    print('Audio rehearsal passed: no video takes, exact PCM duration, measured onset/cut continuation/silent tail, immutable source snapshot and rejected-speech gate.')
    # Non-destructive picture in-points must select the later color segment in
    # actual encoded output, rather than merely changing timeline metadata.
    trim_episode=tool('create_episode',series_id=series['id'],title='Picture trim fixture')
    trim_shot=tool('create_shot',episode_id=trim_episode['id'],title='Choose blue',prompt='Explicit red/blue trim fixture',duration=1.)
    colors=root/'colors.mp4'
    subprocess.run([config['ffmpeg'],'-v','error','-f','lavfi','-i','color=c=red:size=128x224:rate=24:duration=2','-f','lavfi','-i','color=c=blue:size=128x224:rate=24:duration=2','-filter_complex','[0:v][1:v]concat=n=2:v=1:a=0[v]','-map','[v]','-c:v','libx264',str(colors)],check=True)
    color_hash=hashlib.sha256(colors.read_bytes()).hexdigest()
    db.execute("INSERT INTO assets VALUES('colors','fixture',?,?,128,224,4,?,'approved')",(str(colors),color_hash,json.dumps(dict(kind='synthetic_test_fixture'))));db.commit()
    tool('assign_shot_asset',shot_id=trim_shot['id'],asset_id='colors')
    trim=dict(shot_id=trim_shot['id'],source_start=2.25,duration=1.25,expected_source_start=0.,expected_duration=1.,expected_asset_id='colors')
    selected=tool('trim_shot',trim=trim)
    assert selected['source_start']==2.25 and selected['duration']==1.25
    stale=request('tools/call',dict(name='dramafactory_trim_shot',arguments=dict(trim=trim)))
    assert stale.get('isError'),stale
    bad=dict(trim,source_start=3.75,expected_source_start=2.25,expected_duration=1.25)
    outside=request('tools/call',dict(name='dramafactory_trim_shot',arguments=dict(trim=bad)))
    assert outside.get('isError'),outside
    trimmed=wait(tool('export_episode',episode_id=trim_episode['id']))
    output=next(a for a in tool('list_assets') if a['id']==trimmed['output_id'])
    pixel=subprocess.check_output([config['ffmpeg'],'-v','error','-i',output['path'],'-frames:v','1','-vf','scale=1:1','-pix_fmt','rgb24','-f','rawvideo','-'])
    assert len(pixel)==3 and pixel[2]>180 and pixel[0]<40,pixel
    assert abs(output['duration']-1.25)<.05 and output['metadata']['timeline']['clips'][0]['source_start']==2.25
    assert hashlib.sha256(colors.read_bytes()).hexdigest()==color_hash
    print('Picture trim passed: later blue segment appears in actual MP4, exact trimmed duration, range/stale-draft gates, immutable source and snapshot in-point.')
    # Picture fades change frames while leaving the exact episode/audio timebase.
    tool('save_audio_cue',episode_id=trim_episode['id'],asset_id=imported['id'],name='Continuous sound under picture fades',kind='music',start=0.,source_start=0.,duration=1.25,gain_db=-6.,fade_in=0.,fade_out=0.)
    plain_sound=wait(tool('export_episode',episode_id=trim_episode['id']))
    plain_sound_asset=next(a for a in tool('list_assets') if a['id']==plain_sound['output_id'])
    zero=dict(fade_in=0.,fade_out=0.);fades=dict(fade_in=.4,fade_out=.4)
    assert tool('get_picture_fades',shot_id=trim_shot['id'])==zero
    edit=dict(shot_id=trim_shot['id'],fades=fades,expected=zero)
    assert tool('set_picture_fades',edit=edit)==fades and tool('set_picture_fades',edit=edit)==fades
    stale_fades=request('tools/call',dict(name='dramafactory_set_picture_fades',arguments=dict(edit=dict(edit,fades=zero))))
    assert stale_fades.get('isError'),stale_fades
    excessive=request('tools/call',dict(name='dramafactory_set_picture_fades',arguments=dict(edit=dict(edit,fades=dict(fade_in=1.,fade_out=1.),expected=fades))))
    assert excessive.get('isError'),excessive
    too_short=request('tools/call',dict(name='dramafactory_trim_shot',arguments=dict(trim=dict(trim,source_start=2.25,duration=.5,expected_source_start=2.25,expected_duration=1.25))))
    assert too_short.get('isError'),too_short
    faded_job=wait(tool('export_episode',episode_id=trim_episode['id']))
    faded=next(a for a in tool('list_assets') if a['id']==faded_job['output_id'])
    def rgb_at(path,second):
        return subprocess.check_output([config['ffmpeg'],'-v','error','-i',path,'-ss',str(second),'-frames:v','1','-vf','scale=1:1','-pix_fmt','rgb24','-f','rawvideo','-'])
    assert max(rgb_at(faded['path'],0))<15
    assert rgb_at(faded['path'],.6)[2]>180
    assert max(rgb_at(faded['path'],1.2))<70
    assert abs(faded['duration']-plain_sound_asset['duration'])<.01
    audio=[]
    for asset in [plain_sound_asset,faded]:
        values=array.array('f');values.frombytes(subprocess.check_output([config['ffmpeg'],'-v','error','-i',asset['path'],'-vn','-ac','1','-ar','48000','-f','f32le','-']));audio.append(values)
    assert len(audio[0])==len(audio[1])
    assert max(abs(a-b) for a,b in zip(*audio))<.0001
    assert faded['metadata']['timeline']['clips'][0]['picture_fades']==fades
    assert hashlib.sha256(colors.read_bytes()).hexdigest()==color_hash
    print('Picture fades passed: actual black/blue/near-black frames, unchanged duration and decoded audio, immutable source, idempotency/conflict/bounds/trim gates.')
    # Hold the same OS queue lock as the worker to deterministically edit a
    # pending line away and back before real inference. An older revision must
    # retain its output as a take, but must not select it for the newer draft.
    import fcntl
    with (root/'worker.lock').open('a+') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        stale_job=tool('generate_dialogue',shot_id=bridge_shots[1]['id'],text='This answer belongs to the earlier draft.',voice='am_michael',speed=1.)
        pending=tool('list_dialogue',shot_id=bridge_shots[1]['id'])[0]
        assert tool('get_job_status',job_id=stale_job['id'])['status']=='queued'
        def save_words(line,text):
            payload={key:line[key] for key in ['id','shot_id','text','voice','speed','timeline_start','revision']}
            payload['text']=text
            return tool('save_dialogue_draft',line=payload)
        edited=save_words(pending,'An entirely different answer.')
        restored=save_words(edited,pending['text'])
        fcntl.flock(lock,fcntl.LOCK_UN)
    # The already-spawned consumer is waiting for the lock.
    stale_result=wait(stale_job)
    assert stale_result['output_id'] in [asset['id'] for asset in tool('list_assets')]
    current=tool('list_dialogue',shot_id=bridge_shots[1]['id'])[0]
    assert current['revision']==restored['revision'] and current['asset_id'] is None,current
    with (root/'worker.lock').open('a+') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        fresh_job=tool('generate_dialogue',shot_id=current['shot_id'],line_id=current['id'],text=current['text'],voice=current['voice'],speed=current['speed'])
        tool('set_dialogue_timing',line_id=current['id'],timeline_start=.1)
        fcntl.flock(lock,fcntl.LOCK_UN)
    fresh=wait(fresh_job)
    assert tool('list_dialogue',shot_id=current['shot_id'])[0]['asset_id']==fresh['output_id']
    print('Dialogue revision safety passed: real queued ABA edit preserves old output without selecting it; timing-only changes preserve pending speech validity.')
    # Two independent producers race to resume the same saved episode. Hold the
    # real OS worker lock while testing queue convergence and cancellation retry.
    import uuid
    batch_episode=tool('create_episode',series_id=series['id'],title='Resumable dialogue batch')
    batch_shots=[tool('create_shot',episode_id=batch_episode['id'],title=f'Batch {i}',prompt='Explicit batch fixture',duration=3.) for i in range(2)]
    for i,shot in enumerate(batch_shots):
        tool('save_dialogue_draft',line=dict(id=str(uuid.uuid4()),shot_id=shot['id'],text=['I can tell you the truth.','Then I can listen.'][i],voice='af_heart',speed=1.,timeline_start=None,revision=0))
    second=subprocess.Popen([str(binary),'--mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=env)
    def second_request(method,params,rpc_id):
        second.stdin.write(json.dumps(dict(jsonrpc='2.0',id=rpc_id,method=method,params=params))+'\n');second.stdin.flush()
        assert select.select([second.stdout],[],[],20)[0]
        return json.loads(second.stdout.readline())['result']
    second_request('initialize',dict(protocolVersion='2025-06-18',capabilities={},clientInfo=dict(name='Concurrent producer',version='v002')),1)
    second.stdin.write(json.dumps(dict(jsonrpc='2.0',method='notifications/initialized'))+'\n');second.stdin.flush()
    batch_params=dict(name='dramafactory_produce_episode_dialogue',arguments=dict(episode_id=batch_episode['id']))
    with (root/'worker.lock').open('a+') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        sequence+=1
        p.stdin.write(json.dumps(dict(jsonrpc='2.0',id=sequence,method='tools/call',params=batch_params))+'\n');p.stdin.flush()
        second.stdin.write(json.dumps(dict(jsonrpc='2.0',id=2,method='tools/call',params=batch_params))+'\n');second.stdin.flush()
        batches=[]
        for client in [p,second]:
            assert select.select([client.stdout],[],[],20)[0]
            response=json.loads(client.stdout.readline())['result'];assert not response.get('isError'),response
            batches.append(json.loads(response['content'][0]['text']))
        assert sorted(b['newly_queued'] for b in batches)==[0,2],batches
        assert sorted(b['reused_pending'] for b in batches)==[0,2],batches
        job_ids={job['id'] for job in batches[0]['jobs']}
        assert job_ids=={job['id'] for job in batches[1]['jobs']}
        repeated=tool('produce_episode_dialogue',episode_id=batch_episode['id'])
        assert repeated['newly_queued']==0 and repeated['reused_pending']==2
        tool('cancel_job',job_id=repeated['jobs'][0]['id'])
        resumed=tool('produce_episode_dialogue',episode_id=batch_episode['id'])
        assert resumed['retried']==1 and resumed['reused_pending']==1
        assert {job['id'] for job in resumed['jobs']}==job_ids
        fcntl.flock(lock,fcntl.LOCK_UN)
    for job in resumed['jobs']:wait(job)
    complete=tool('produce_episode_dialogue',episode_id=batch_episode['id'])
    assert complete['ready_lines']==2 and not complete['jobs'],complete
    old=tool('list_dialogue',shot_id=batch_shots[0]['id'])[0]
    save_words(old,'The next draft is different.')
    repair=tool('produce_episode_dialogue',episode_id=batch_episode['id'])
    assert repair['ready_lines']==1 and repair['newly_queued']==1,repair
    wait(repair['jobs'][0])
    selected=tool('list_dialogue',shot_id=batch_shots[0]['id'])[0]
    selected_asset=next(asset for asset in tool('list_assets') if asset['id']==selected['asset_id'])
    source_path=Path(selected_asset['path']);original=source_path.read_bytes()
    source_path.write_bytes(original+b'explicit corruption fixture')
    repair=tool('produce_episode_dialogue',episode_id=batch_episode['id'])
    source_path.write_bytes(original)
    assert repair['ready_lines']==1 and repair['newly_queued']==1,repair
    wait(repair['jobs'][0])
    assert tool('produce_episode_dialogue',episode_id=batch_episode['id'])['ready_lines']==2
    second.stdin.close();assert second.wait(timeout=10)==0
    print('Episode speech orchestration passed: concurrent MCP producers converge on two jobs, repeated resume reuses pending jobs, canceled attempt retries in place, selected takes are skipped, edits and corrupt selections regenerate only the affected line.')
    move_episode=tool('create_episode',series_id=series['id'],title='Pending speech relocation fixture')
    move_shots=[tool('create_shot',episode_id=move_episode['id'],title=title,prompt='Explicit relocation fixture',duration=3.) for title in ['Source','Destination']]
    with (root/'worker.lock').open('a+') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        pending=tool('generate_dialogue',shot_id=move_shots[0]['id'],text='Keep my voice when the picture changes.',voice='af_heart',speed=1.)
        original_line=tool('list_dialogue',shot_id=move_shots[0]['id'])[0]
        edit=dict(line_id=original_line['id'],target_shot_id=move_shots[1]['id'],expected_shot_id=move_shots[0]['id'],expected_revision=original_line['revision'])
        moved=tool('move_dialogue',edit=edit)
        assert moved['timeline_start']==0. and moved['id']==original_line['id'] and not tool('list_dialogue',shot_id=move_shots[0]['id'])
        assert tool('move_dialogue',edit=edit)==moved
        fcntl.flock(lock,fcntl.LOCK_UN)
    resumed=tool('produce_episode_dialogue',episode_id=move_episode['id'])
    assert resumed['newly_queued']==0 and resumed['reused_pending']==1
    wait(pending)
    selected=tool('list_dialogue',shot_id=move_shots[1]['id'])[0]
    assert selected['asset_id']==tool('get_job_status',job_id=pending['id'])['output_id']
    assert selected['timeline_start']==0. and tool('produce_episode_dialogue',episode_id=move_episode['id'])['ready_lines']==1
    print('Dialogue move passed: actual pending speech survives source-shot move without regeneration; absolute placement, ID and voice selection preserved.')
    # Deterministically inject cancellation at validated-asset registration,
    # after actual FFmpeg/Kokoro work and before the terminal status update.
    # Trigger exists only in this disposable database, never in the real studio.
    db.executescript("CREATE TABLE cancellation_targets(kind TEXT PRIMARY KEY); CREATE TRIGGER cancel_at_registration AFTER INSERT ON assets WHEN EXISTS(SELECT 1 FROM jobs j JOIN cancellation_targets t ON t.kind=j.kind WHERE j.id=NEW.job_id AND j.status='running') BEGIN UPDATE jobs SET cancel_requested=1 WHERE id=NEW.job_id; END;")
    def terminal(job,expected):
        deadline=time.monotonic()+90
        while time.monotonic()<deadline:
            status=tool('get_job_status',job_id=job['id'])
            if status['status'] in ['completed','canceled','failed','blocked']:
                assert status['status']==expected,status
                return status
            time.sleep(.1)
        raise TimeoutError('Completion cancellation fixture timed out')
    for kind,tool_name,args in [('export','export_episode',dict(episode_id=episode['id'])),('rehearsal','export_dialogue_rehearsal',dict(episode_id=move_episode['id'])),('dialogue','generate_dialogue',dict(shot_id=move_shots[0]['id'],text='Keep this canceled output for review.',voice='af_heart',speed=1.))]:
        db.execute('INSERT INTO cancellation_targets VALUES(?)',(kind,));db.commit()
        canceled_job=terminal(tool(tool_name,**args),'canceled')
        output=next(a for a in tool('list_assets') if a['id']==canceled_job['output_id'])
        assert Path(output['path']).is_file() and db.execute('SELECT cancel_requested FROM jobs WHERE id=?',(canceled_job['id'],)).fetchone()[0] and canceled_job['error'],canceled_job
        assert hashlib.sha256(Path(output['path']).read_bytes()).hexdigest()==output['hash']
        if kind=='dialogue':
            line_id=output['metadata']['request']['line_id']
            assert next(line for line in tool('list_dialogue',shot_id=move_shots[0]['id']) if line['id']==line_id)['asset_id'] is None
        db.execute('DELETE FROM cancellation_targets');db.commit()
        # Hold the real queue lock briefly to inspect retry's queued snapshot.
        with (root/'worker.lock').open('a+') as queue_lock:
            deadline=time.monotonic()+5
            while True:
                try:fcntl.flock(queue_lock,fcntl.LOCK_EX|fcntl.LOCK_NB);break
                except BlockingIOError:
                    assert time.monotonic()<deadline
                    time.sleep(.02)
            tool('retry_job',job_id=canceled_job['id'])
            queued=tool('get_job_status',job_id=canceled_job['id'])
            assert queued['status']=='queued' and queued['output_id'] is None,queued
            assert hashlib.sha256(Path(output['path']).read_bytes()).hexdigest()==output['hash']
            fcntl.flock(queue_lock,fcntl.LOCK_UN)
        retried=terminal(canceled_job,'completed')
        assert retried['attempts']==2 and retried['output_id']!=output['id']
        assert any(a['id']==output['id'] for a in tool('list_assets'))
    print('Late cancellation passed: real export/rehearsal/speech retain validated outputs but stay canceled; speech unselected; retries clear active output, complete attempt two and preserve prior assets.')
    # Startup recovery preserves cancellation intent and all attempt files.
    recovery=[]
    for cancel in [0,1]:
        job_id=str(uuid.uuid4());attempt_dir=root/'jobs'/job_id/'attempt-1';attempt_dir.mkdir(parents=True)
        sentinel=attempt_dir/'preserve.log';sentinel.write_text('Explicit interrupted-attempt fixture')
        db.execute("INSERT INTO jobs(id,prompt,status,kind,request,cancel_requested,attempts) VALUES(?,'Interrupted fixture','running','export','{}',?,1)",(job_id,cancel));recovery.append((job_id,cancel,sentinel))
    db.commit()
    subprocess.run([str(binary),'--worker'],env=env,check=True,timeout=20,capture_output=True)
    for job_id,cancel,sentinel in recovery:
        recovered=tool('get_job_status',job_id=job_id)
        assert recovered['status']==('canceled' if cancel else 'failed') and recovered['attempts']==1,recovered
        assert sentinel.read_text()=='Explicit interrupted-attempt fixture'
    print('Startup recovery passed: interrupted canceled jobs stay canceled, other abandoned jobs fail for explicit retry, all attempt files preserved.')
    p.stdin.close();assert p.wait(timeout=10)==0
    db.close()
    print("Production integration passed: real Kokoro WAV, managed assets, dialogue mixing, approved-shot assembly, 1080x1920/24fps MP4, duration and full decode validation.")
