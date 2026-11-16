"""Real MCP/FFmpeg export of continuation metadata using retained footage.
No model inference and no approval change to the original rejected benchmark.
A synthetic 6s tone verifies routing, not acting or lip synchronization.
"""
from pathlib import Path
import array,hashlib,json,math,os,select,sqlite3,subprocess,tempfile,time,uuid
repo=Path(__file__).resolve().parents[1]
retained=json.loads((repo/'benchmarks/local/native-performance-mcp-state.json').read_text())['asset']
video=Path(retained['path']);snapshot=retained['metadata']['request'];ffmpeg=snapshot['setup']['ffmpeg']
assert hashlib.sha256(video.read_bytes()).hexdigest()==retained['hash']
with tempfile.TemporaryDirectory(prefix='df-performance-windows-') as temporary:
    root=Path(temporary);database=root/'studio.sqlite'
    client=subprocess.Popen([str(repo/'target/release/dramafactory'),'--mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=dict(os.environ,DRAMAFACTORY_DATABASE=str(database)))
    sequence=0
    def rpc(method,params):
        global sequence
        sequence+=1;client.stdin.write(json.dumps(dict(jsonrpc='2.0',id=sequence,method=method,params=params))+'\n');client.stdin.flush()
        assert select.select([client.stdout],[],[],20)[0],method
        r=json.loads(client.stdout.readline());assert r['id']==sequence,r
        return r['result']
    def call(tool_name,fail=False,**args):
        r=rpc('tools/call',dict(name='dramafactory_'+tool_name,arguments=args))
        if fail:assert r.get('isError'),r;return r
        assert not r.get('isError'),r
        return json.loads(r['content'][0]['text'])
    rpc('initialize',dict(protocolVersion='2025-06-18',capabilities={},clientInfo=dict(name='Continuation export fixture',version='v002')))
    client.stdin.write(json.dumps(dict(jsonrpc='2.0',method='notifications/initialized'))+'\n');client.stdin.flush()
    db=sqlite3.connect(database);db.execute("INSERT INTO settings VALUES('worker',?)",(json.dumps(dict(ffmpeg=ffmpeg)),));db.commit()
    series=call('create_series',**{'name':'Explicit continuation fixture'});episode=call('create_episode',series_id=series['id'],title='Technical routing only')
    shots=[call('create_shot',episode_id=episode['id'],title=title,prompt='Retained real-footage technical fixture',duration=3.) for title in ['Start','Continuation','Reaction']]
    wave=root/'six-seconds.wav';subprocess.run([ffmpeg,'-v','error','-f','lavfi','-i','sine=frequency=440:sample_rate=48000','-t','6','-ac','2','-c:a','pcm_s16le',str(wave)],check=True)
    voice_hash=hashlib.sha256(wave.read_bytes()).hexdigest()
    db.execute("INSERT INTO assets VALUES('voice','fixture',?,?,0,0,6,?,'unreviewed')",(str(wave),voice_hash,json.dumps(dict(kind='synthetic_audio_fixture'))))
    db.execute("INSERT INTO assets VALUES('generic-picture','fixture',?,?,?,?,?,?,'approved')",(str(video),retained['hash'],retained['width'],retained['height'],retained['duration'],json.dumps(dict(kind='retained_footage_technical_fixture'))));db.commit()
    line=call('save_dialogue_draft',line=dict(id=str(uuid.uuid4()),shot_id=shots[0]['id'],text='Long audio-routing fixture, not an acting result.',voice='am_michael',speed=1.,timeline_start=.5,revision=0))
    db.execute("UPDATE dialogue SET asset_id='voice' WHERE id=?",(line['id'],));db.commit();line=call('list_dialogue',shot_id=shots[0]['id'])[0]
    native=dict(snapshot,shot_id=shots[1]['id'],source_line=line,audio=str(wave),audio_hash=voice_hash,audio_start=0.,audio_window=dict(source_start=2.5),frames=73)
    db.execute("INSERT INTO assets VALUES('continuation-picture','fixture',?,?,?,?,?,?,'approved')",(str(video),retained['hash'],retained['width'],retained['height'],retained['duration'],json.dumps(dict(kind='performance',request=native,explicit_metadata_fixture=True))));db.commit()
    for shot in shots:call('assign_shot_asset',shot_id=shot['id'],asset_id='continuation-picture' if shot==shots[1] else 'generic-picture')
    call('set_dialogue_timing',line_id=line['id'],timeline_start=.75)
    call('export_episode',fail=True,episode_id=episode['id'])
    call('assign_shot_asset',fail=True,shot_id=shots[1]['id'],asset_id='continuation-picture')
    call('set_dialogue_timing',line_id=line['id'],timeline_start=.5)
    job=call('export_episode',episode_id=episode['id']);deadline=time.monotonic()+90
    while True:
        state=call('get_job_status',job_id=job['id']);assert state['status'] not in ['failed','blocked','canceled'],state
        if state['status']=='completed':break
        assert time.monotonic()<deadline;time.sleep(.1)
    output=next(a for a in call('list_assets') if a['id']==state['output_id']);assert abs(output['duration']-9.)<.05
    def pcm(path):
        result=array.array('f');result.frombytes(subprocess.check_output([ffmpeg,'-v','error','-i',str(path),'-vn','-ac','1','-ar','48000','-f','f32le','-']));return result
    source=pcm(wave);mixed=pcm(output['path']);active=mixed[24000:24000+len(source)]
    ratio=math.sqrt(sum(v*v for v in active)/sum(v*v for v in source));cosine=sum(a*b for a,b in zip(active,source))/math.sqrt(sum(v*v for v in active)*sum(v*v for v in source))
    assert .97<ratio<1.03 and cosine>.98,(ratio,cosine)
    assert max(abs(v) for v in mixed[7*48000:8*48000])<.001
    assert len(output['metadata']['timeline']['cues'])==1
    assert hashlib.sha256(video.read_bytes()).hexdigest()==retained['hash'] and hashlib.sha256(wave.read_bytes()).hexdigest()==voice_hash
    client.stdin.close();assert client.wait(timeout=10)==0;db.close()
    print(f'Continuation export passed: actual MCP/FFmpeg 9s cut, long source audio mixed once across cuts (RMS ratio {ratio:.4f}, cosine {cosine:.5f}), stale-window gates, originals unchanged. Retained footage fixture only; no generated-window or lip-sync approval.')
