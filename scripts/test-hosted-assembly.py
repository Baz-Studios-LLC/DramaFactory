"""Hosted segment selection and real FFmpeg assembly in a disposable database.
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
    import uuid, wave, struct, math, copy
    series=tool('create_series',name='Hosted assembly fixtures',premise='Synthetic test only')
    episode=tool('create_episode',series_id=series['id'],title='Two generated-soundtrack fixtures')
    plan=tool('get_generation_plan',episode_id=episode['id'])
    plan['segments'][0]['prompt']='Red picture with 440 Hz fixture'
    second=copy.deepcopy(plan['segments'][0]);second['id']=str(uuid.uuid4());second['prompt']='Blue picture with 880 Hz fixture';plan['segments'].append(second)
    plan=tool('save_generation_plan',plan=plan)
    originals=[];assets=[]
    for index,(color,frequency) in enumerate([('red',440),('blue',880)]):
        clip=root/f'fixture-{index}.mp4'
        subprocess.run([config['ffmpeg'],'-v','error','-f','lavfi','-i',f'color=c={color}:s=128x224:r=24','-f','lavfi','-i',f'sine=frequency={frequency}:sample_rate=48000','-t','0.5','-c:v','libx264','-pix_fmt','yuv420p','-c:a','aac',str(clip)],check=True)
        digest=hashlib.sha256(clip.read_bytes()).hexdigest();originals.append((clip,digest))
        job=str(uuid.uuid4());asset=str(uuid.uuid4());assets.append(asset)
        segment=plan['segments'][index]
        db.execute("INSERT INTO jobs(id,prompt,status,kind,request) VALUES(?,?,'completed','openrouter_video','{}')",(job,'Explicit synthetic completed source fixture'))
        db.execute('INSERT INTO segment_generations VALUES(?,?,?,?,?)',(job,episode['id'],segment['id'],plan['revision'],json.dumps(segment,separators=(',',':'))))
        db.execute("INSERT INTO assets VALUES(?,?,?,?,128,224,0.5,?,'unreviewed')",(asset,job,str(clip),digest,json.dumps(dict(kind='synthetic_hosted_assembly_fixture'))));db.commit()
        unreviewed=request('tools/call',dict(name='dramafactory_select_segment_take',arguments=dict(episode_id=episode['id'],segment_id=segment['id'],asset_id=asset,expected_asset_id=None)))
        assert unreviewed.get('isError'),unreviewed
        tool('review_asset',asset_id=asset,review='approved')
        tool('select_segment_take',episode_id=episode['id'],segment_id=segment['id'],asset_id=asset,expected_asset_id=None)
    job=tool('export_hosted_episode',episode_id=episode['id'])
    deadline=time.monotonic()+90
    while time.monotonic()<deadline:
        result=tool('get_job_status',job_id=job['id'])
        if result['status'] in ['failed','canceled','blocked']:raise AssertionError(result)
        if result['status']=='completed':break
        time.sleep(.2)
    else:raise TimeoutError('Hosted fixture assembly timed out')
    output=next(asset for asset in tool('list_assets') if asset['id']==result['output_id'])
    assert abs(output['duration']-1)<.15 and output['width']==720 and output['height']==1280,output
    wav=root/'output.wav';subprocess.run([config['ffmpeg'],'-v','error','-i',output['path'],'-vn','-ac','1','-ar','48000','-c:a','pcm_s16le',str(wav)],check=True)
    with wave.open(str(wav)) as source:
        samples=struct.unpack('<'+'h'*source.getnframes(),source.readframes(source.getnframes()))
    frequencies=[]
    for start,end in [(.1,.4),(.6,.9)]:
        window=samples[int(start*48000):int(end*48000)];rms=math.sqrt(sum(sample*sample for sample in window)/len(window));assert rms>500,rms
        crossings=sum(left<=0<right for left,right in zip(window,window[1:]));frequencies.append(crossings/(end-start))
    assert abs(frequencies[0]-440)<20 and abs(frequencies[1]-880)<20,frequencies
    for moment,channel in [(.25,0),(.75,2)]:
        raw=subprocess.run([config['ffmpeg'],'-v','error','-ss',str(moment),'-i',output['path'],'-vf','scale=1:1','-frames:v','1','-pix_fmt','rgb24','-f','rawvideo','-'],check=True,capture_output=True).stdout
        assert raw[channel]>150 and raw[channel]>max(raw[(channel+1)%3],raw[(channel+2)%3])+80,list(raw)
    for clip,digest in originals:assert hashlib.sha256(clip.read_bytes()).hexdigest()==digest
    plan['segments'][0]['prompt']='Changed after take selection';tool('save_generation_plan',plan=plan)
    stale=request('tools/call',dict(name='dramafactory_export_hosted_episode',arguments=dict(episode_id=episode['id'])))
    assert stale.get('isError'),stale
    p.stdin.close();assert p.wait(timeout=10)==0
    print(json.dumps(dict(status='passed',duration=output['duration'],soundtrack_frequencies=frequencies,checks=['unreviewed selection rejected','original picture order','each segment soundtrack preserved once','source hashes unchanged','stale segment export rejected']),indent=2))
