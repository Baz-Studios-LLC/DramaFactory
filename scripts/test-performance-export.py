"""Export a retained native-performance take through real MCP in a disposable DB.
Fixture approval permits technical export only; it never changes source review.
Requires benchmarks/local/native-performance-mcp-state.json from the real test.
No additional model inference, writing to the source DB, or source modification.
"""
from pathlib import Path
import json,os,sqlite3,tempfile,subprocess,select,time,hashlib,array,math,wave
repo=Path(__file__).resolve().parents[1]
retained=json.loads((repo/'benchmarks/local/native-performance-mcp-state.json').read_text())
source=retained['asset'];request_snapshot=source['metadata']['request']
video=Path(source['path']);voice=Path(request_snapshot['audio']);ffmpeg=request_snapshot['setup']['ffmpeg']
assert hashlib.sha256(video.read_bytes()).hexdigest()==source['hash']
assert hashlib.sha256(voice.read_bytes()).hexdigest()==request_snapshot['audio_hash']
with wave.open(str(voice),'rb') as waveform:voice_seconds=waveform.getnframes()/waveform.getframerate()
binary=Path(os.environ.get('DRAMAFACTORY_BINARY',repo/'target/release/dramafactory'))
with tempfile.TemporaryDirectory(prefix='df-native-export-') as temporary:
    database=Path(temporary)/'studio.sqlite'
    client=subprocess.Popen([str(binary),'--mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=dict(os.environ,DRAMAFACTORY_DATABASE=str(database)))
    sequence=0
    def rpc(method,params):
        global sequence
        sequence+=1;client.stdin.write(json.dumps(dict(jsonrpc='2.0',id=sequence,method=method,params=params))+'\n');client.stdin.flush()
        assert select.select([client.stdout],[],[],20)[0],method
        response=json.loads(client.stdout.readline());assert response['id']==sequence,response
        return response['result']
    def call(tool_name,fail=False,**arguments):
        result=rpc('tools/call',dict(name='dramafactory_'+tool_name,arguments=arguments))
        if fail:assert result.get('isError'),result;return result
        assert not result.get('isError'),result
        return json.loads(result['content'][0]['text'])
    rpc('initialize',dict(protocolVersion='2025-06-18',capabilities={},clientInfo=dict(name='Native export fixture',version='v002')))
    client.stdin.write(json.dumps(dict(jsonrpc='2.0',method='notifications/initialized'))+'\n');client.stdin.flush()
    db=sqlite3.connect(database);db.execute("INSERT INTO settings VALUES('worker',?)",(json.dumps(dict(ffmpeg=ffmpeg)),));db.commit()
    series=call('create_series',name='Explicit disposable export fixture')
    episode=call('create_episode',series_id=series['id'],title='Native performance technical fixture')
    shot=call('create_shot',episode_id=episode['id'],title='Retained native footage',prompt='Explicit real-media export fixture, no acting-quality claim',duration=3.2)
    line=call('save_dialogue_draft',line=dict(id=str(__import__('uuid').uuid4()),shot_id=shot['id'],text=request_snapshot['source_line']['text'],voice=request_snapshot['source_line']['voice'],speed=1.,timeline_start=None,revision=0))
    db.execute("INSERT INTO assets VALUES('picture','fixture',?,?,?,?,?,?, 'approved')",(str(video),source['hash'],source['width'],source['height'],source['duration'],json.dumps(source['metadata'])))
    db.execute("INSERT INTO assets VALUES('speech','fixture',?,?,0,0,?,?,'unreviewed')",(str(voice),request_snapshot['audio_hash'],voice_seconds,json.dumps(dict(kind='dialogue'))))
    db.execute("UPDATE dialogue SET asset_id='speech' WHERE id=?",(line['id'],));db.commit()
    call('assign_shot_asset',shot_id=shot['id'],asset_id='picture')
    call('set_dialogue_visibility',line_id=line['id'],speaker_visible=False,expected_visible=True)
    assert call('list_dialogue',shot_id=shot['id'])[0]['asset_id']=='speech'
    rejected=call('assign_shot_asset',fail=True,shot_id=shot['id'],asset_id='picture')
    assert 'off screen' in rejected['content'][0]['text'],rejected
    rejected=call('generate_performance',fail=True,shot_id=shot['id'],audio_asset_id='speech')
    assert 'off screen' in rejected['content'][0]['text'],rejected
    rejected=call('synchronize_lips',fail=True,shot_id=shot['id'],video_asset_id='picture',audio_asset_id='speech')
    assert 'off screen' in rejected['content'][0]['text'],rejected
    linked_metadata=dict(kind='lipsync',request=dict(shot_id=shot['id'],audio_asset_id='speech',video_asset_id='picture'))
    db.execute("INSERT INTO assets VALUES('linked-lipsync-fixture','fixture',?,?,?,?,?,?,'approved')",(str(video),source['hash'],source['width'],source['height'],source['duration'],json.dumps(linked_metadata)));db.commit()
    rejected=call('assign_shot_asset',fail=True,shot_id=shot['id'],asset_id='linked-lipsync-fixture')
    assert 'off screen' in rejected['content'][0]['text'],rejected
    call('export_episode',fail=True,episode_id=episode['id'])
    call('set_dialogue_visibility',line_id=line['id'],speaker_visible=True,expected_visible=False)
    # An additional off-screen voice must not be mistaken for the visible actor.
    phone=call('save_dialogue_draft',line=dict(id=str(__import__('uuid').uuid4()),shot_id=shot['id'],text='An off-screen caller.',voice='af_heart',speed=1.,timeline_start=2.5,revision=0))
    call('set_dialogue_visibility',line_id=phone['id'],speaker_visible=False,expected_visible=True)
    call('assign_shot_asset',shot_id=shot['id'],asset_id='picture')
    db.execute('DELETE FROM dialogue WHERE id=?',(phone['id'],));db.commit()
    call('set_dialogue_timing',line_id=line['id'],timeline_start=.25)
    call('export_episode',fail=True,episode_id=episode['id'])
    call('set_dialogue_timing',line_id=line['id'],timeline_start=None)
    line=call('list_dialogue',shot_id=shot['id'])[0];line.pop('asset_id');line['text']='A different sentence.'
    line=call('save_dialogue_draft',line=line)
    db.execute("UPDATE dialogue SET asset_id='speech' WHERE id=?",(line['id'],));db.commit()
    call('assign_shot_asset',fail=True,shot_id=shot['id'],asset_id='picture')
    call('export_episode',fail=True,episode_id=episode['id'])
    line.pop('asset_id');line['text']=request_snapshot['source_line']['text'];call('save_dialogue_draft',line=line)
    db.execute("UPDATE dialogue SET asset_id='speech' WHERE id=?",(line['id'],));db.commit()
    call('set_caption_mode',episode_id=episode['id'],burn_in=True)
    job=call('export_episode',episode_id=episode['id']);deadline=time.monotonic()+90
    while time.monotonic()<deadline:
        status=call('get_job_status',job_id=job['id']);assert status['status'] not in ['failed','canceled','blocked'],status
        if status['status']=='completed':break
        time.sleep(.1)
    else:raise TimeoutError(job)
    result=next(asset for asset in call('list_assets') if asset['id']==status['output_id'])
    assert result['width']==1080 and result['height']==1920 and abs(result['duration']-3.2)<.05
    assert result['metadata']['captions_burned'] is True
    subprocess.run([ffmpeg,'-v','error','-xerror','-i',result['path'],'-f','null','-'],check=True)
    def pcm(path):
        values=array.array('f');values.frombytes(subprocess.check_output([ffmpeg,'-v','error','-i',str(path),'-vn','-ac','1','-ar','24000','-f','f32le','-']));return values
    expected=pcm(voice);actual=pcm(result['path']);start,end=2400,min(len(expected),len(actual))-2400
    a=expected[start:end];b=actual[start:end]
    rms_ratio=math.sqrt(sum(x*x for x in b)/sum(x*x for x in a))
    cosine=sum(x*y for x,y in zip(a,b))/math.sqrt(sum(x*x for x in a)*sum(y*y for y in b))
    assert .97<rms_ratio<1.03 and cosine>.98,(rms_ratio,cosine)
    assert hashlib.sha256(video.read_bytes()).hexdigest()==source['hash']
    assert hashlib.sha256(voice.read_bytes()).hexdigest()==request_snapshot['audio_hash']
    client.stdin.close();assert client.wait(timeout=10)==0;db.close()
    print(f'Native export passed: real MCP 1080x1920/24fps MP4 and captions, source preservation, stale speech/timing gates, speech mixed once (RMS ratio {rms_ratio:.4f}, waveform cosine {cosine:.5f}). Not a lip-sync quality score.')
