"""Exercise actual queue/worker reference-integrity failures without model inference.
All input bytes and checkpoint filenames are explicit disposable fixtures.
"""
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile

repo=Path(__file__).resolve().parents[1]
binary=Path(os.environ.get('DRAMAFACTORY_BINARY',repo/'target/release/dramafactory'))
with tempfile.TemporaryDirectory(prefix='df-reference-integrity-') as temporary:
    root=Path(temporary);database=root/'studio.sqlite'
    env=dict(os.environ,DRAMAFACTORY_DATABASE=str(database))
    p=subprocess.Popen([str(binary),'--mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=env)
    sequence=0
    def request(method,params):
        global sequence
        sequence+=1;p.stdin.write(json.dumps(dict(jsonrpc='2.0',id=sequence,method=method,params=params))+'\n');p.stdin.flush()
        r=json.loads(p.stdout.readline());assert r['id']==sequence,r
        return r['result']
    def call(name,**args):
        r=request('tools/call',dict(name='dramafactory_'+name,arguments=args));assert not r.get('isError'),r
        return json.loads(r['content'][0]['text'])
    try:
        request('initialize',dict(protocolVersion='2025-06-18',capabilities={},clientInfo=dict(name='Input integrity fixture',version='v002')))
        p.stdin.write(json.dumps(dict(jsonrpc='2.0',method='notifications/initialized'))+'\n');p.stdin.flush()
        reference=root/'fixture-input.png';reference.write_bytes(b'explicit original bytes, not an image-generation result')
        original_hash=hashlib.sha256(reference.read_bytes()).hexdigest()
        image=call('generate_image',prompt='Explicit test fixture',reference=str(reference))
        video=call('queue_render',prompt='Explicit test fixture',image=str(reference))
        assert image['status']==video['status']=='blocked'
    finally:
        p.stdin.close();p.wait(timeout=10)
    with sqlite3.connect(database) as db:
        requests={j['id']:json.loads(db.execute('SELECT request FROM jobs WHERE id=?',(j['id'],)).fetchone()[0]) for j in [image,video]}
        assert requests[image['id']]['reference_hash']==requests[video['id']]['image_hash']==original_hash
        # Ensure setup checks pass, but any accidental inference launch only
        # invokes a disposable sentinel script, never a real model runtime.
        sentinel=root/'inference-sentinel';marker=root/'inference-was-launched'
        sentinel.write_text('#!/bin/sh\ntouch "'+str(marker)+'"\nexit 90\n');sentinel.chmod(0o755)
        models=root/'fixture-models';models.mkdir()
        for name in ['config.json','model.safetensors','t5_encoder.safetensors','vae.safetensors']:(models/name).write_bytes(b'not model weights')
        config=dict(python=str(sentinel),ffmpeg=str(sentinel),model=str(models),image_python=str(sentinel),image_model=str(models))
        db.execute("INSERT INTO settings VALUES('worker',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",(json.dumps(config),))
        db.execute("UPDATE jobs SET status='queued' WHERE id IN (?,?)",(image['id'],video['id']))
    reference.write_bytes(b'explicit changed fixture bytes')
    subprocess.run([str(binary),'--worker'],env=env,check=True,timeout=30,capture_output=True)
    assert not marker.exists(),'Inference started despite changed reference'
    with sqlite3.connect(database) as db:
        for job in [image,video]:
            state=db.execute('SELECT status,error,attempts,output_id FROM jobs WHERE id=?',(job['id'],)).fetchone()
            assert state[0]=='failed' and 'changed after queueing' in state[1] and state[3] is None,state
            copied=root/'jobs'/job['id']/f'attempt-{state[2]}'/'reference-input.png'
            assert copied.read_bytes()==reference.read_bytes()
            assert json.loads(db.execute('SELECT request FROM jobs WHERE id=?',(job['id'],)).fetchone()[0])==requests[job['id']]
    print('Reference integrity passed: real MCP snapshots, both actual worker paths reject changed bytes before inference, copied diagnostics retained, queue requests unchanged.')
