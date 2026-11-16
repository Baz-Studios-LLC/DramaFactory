"""Calibrate unmodified upstream CPU evaluation against its published demo.
Waits for the separately tracked download; never restarts it or generates video.
"""
from pathlib import Path
import json,time,subprocess,os,uuid,hashlib,fcntl,re
repo=Path(__file__).resolve().parents[2]
checkpoint=repo/'benchmarks/local/syncnet-calibration-state.json'
state=dict(status='waiting_for_reference_download',source='joonson/syncnet_python',expected=dict(offset_frames=3,min_distance=5.353,confidence=10.021),scope='Reference calibration only. No generated-clip score or acting-quality approval.')
checkpoint.write_text(json.dumps(state,indent=2)+'\n')
deadline=time.monotonic()+3600
downloads_path=repo/'benchmarks/local/syncnet-downloads.json'
while True:
    try:downloads=json.loads(downloads_path.read_text());break
    except (FileNotFoundError,json.JSONDecodeError):
        if time.monotonic()>deadline:raise TimeoutError('Observe original download session; calibration did not restart it')
        time.sleep(1)
model_root=Path(downloads['root'])
for name,metadata in downloads['files'].items():
    assert hashlib.sha256((model_root/name).read_bytes()).hexdigest()==metadata['sha256']
source=json.loads((repo/'benchmarks/local/syncnet-source.json').read_text())
data=Path.home()/'Library/Application Support/studio.DramaFactory.DramaFactory'
lock=(data/'worker.lock').open('a+');fcntl.flock(lock,fcntl.LOCK_EX)
directory=data/'benchmarks'/('syncnet-calibration-'+str(uuid.uuid4()));directory.mkdir(parents=True)
with __import__('sqlite3').connect(data/'studio.sqlite') as db:ffmpeg=json.loads(db.execute("SELECT value FROM settings WHERE key='worker'").fetchone()[0])['ffmpeg']
tools=directory/'bin';tools.mkdir();(tools/'ffmpeg').symlink_to(ffmpeg)
env=dict(os.environ,PATH=str(tools)+os.pathsep+os.environ.get('PATH',''),OMP_NUM_THREADS='4',VECLIB_MAXIMUM_THREADS='4')
command=[str(repo/'.venv-sync-eval/bin/python'),str(Path(source['path'])/'demo_syncnet.py'),'--initial_model',str(model_root/'syncnet_v2.model'),'--videofile',str(model_root/'example.avi'),'--tmp_dir',str(directory/'frames'),'--reference','upstream-control','--batch_size','8']
state.update(status='running_reference_control',source_revision=source['revision'],directory=str(directory),downloads=downloads['files']);checkpoint.write_text(json.dumps(state,indent=2)+'\n')
started=time.monotonic()
with (directory/'evaluation.log').open('w') as log:
    result=subprocess.run(command,cwd=source['path'],env=env,stdout=log,stderr=subprocess.STDOUT,timeout=900)
text=(directory/'evaluation.log').read_text()
state.update(wall_seconds=time.monotonic()-started,returncode=result.returncode)
if result.returncode==0:
    offset=re.search(r'AV offset:\s*(-?\d+)',text);confidence=re.search(r'Confidence:\s*([0-9.]+)',text);distance=re.search(r'Min dist:\s*([0-9.]+)',text)
    assert offset and confidence and distance,text[-2000:]
    measured=dict(offset_frames=int(offset[1]),confidence=float(confidence[1]),min_distance=float(distance[1]));state['measured']=measured
    passed=measured['offset_frames']==3 and abs(measured['confidence']-10.021)<1.0 and abs(measured['min_distance']-5.353)<0.5
    state['status']='reference_calibration_passed' if passed else 'reference_calibration_mismatch'
else:state['status']='reference_calibration_failed'
checkpoint.write_text(json.dumps(state,indent=2)+'\n');(directory/'calibration.json').write_text(json.dumps(state,indent=2)+'\n')
print(json.dumps(state),flush=True)
