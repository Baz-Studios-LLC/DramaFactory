"""Use calibrated upstream SyncNet + its S3FD crop pipeline on retained takes.
Diagnostics only: scores never approve acting, identity, hands or an episode.
"""
from pathlib import Path
import json,time,uuid,os,subprocess,fcntl,hashlib,re
repo=Path(__file__).resolve().parents[2]
calibration=json.loads((repo/'benchmarks/local/syncnet-calibration-state.json').read_text())
assert calibration['status']=='reference_calibration_passed'
state_path=repo/'benchmarks/local/retained-sync-evaluation-state.json'
state=dict(status='waiting_for_face_model',calibration=calibration['measured'],protocol=dict(fps=25,common_seconds=3.,crop_scale=.4,min_track=25,face_detection_scale=.5,search_window_frames=15),results=[])
state_path.write_text(json.dumps(state,indent=2)+'\n')
deadline=time.monotonic()+3600
while True:
    try:detector=json.loads((repo/'benchmarks/local/syncnet-face-download.json').read_text());break
    except (FileNotFoundError,json.JSONDecodeError):
        if time.monotonic()>deadline:raise TimeoutError('Observe original face-model download; do not restart from this waiter')
        time.sleep(1)
assert hashlib.sha256(Path(detector['path']).read_bytes()).hexdigest()==detector['sha256']
source=json.loads((repo/'benchmarks/local/syncnet-source.json').read_text());source_dir=Path(source['path'])
weights=source_dir/'detectors/s3fd/weights/sfd_face.pth';weights.parent.mkdir(exist_ok=True)
if not weights.exists():weights.symlink_to(detector['path'])
assert hashlib.sha256(weights.read_bytes()).hexdigest()==detector['sha256']
data=Path.home()/'Library/Application Support/studio.DramaFactory.DramaFactory'
lock=(data/'worker.lock').open('a+');fcntl.flock(lock,fcntl.LOCK_EX)
directory=repo/'benchmarks/local'/('sync-evaluation-'+str(uuid.uuid4()));directory.mkdir()
with __import__('sqlite3').connect(data/'studio.sqlite') as db:ffmpeg=json.loads(db.execute("SELECT value FROM settings WHERE key='worker'").fetchone()[0])['ffmpeg']
tools=directory/'bin';tools.mkdir();(tools/'ffmpeg').symlink_to(ffmpeg)
env=dict(os.environ,PATH=str(tools)+os.pathsep+os.environ.get('PATH',''),OMP_NUM_THREADS='4',VECLIB_MAXIMUM_THREADS='4')
native=json.loads((repo/'benchmarks/local/native-performance-mcp-state.json').read_text())
immediate=json.loads((repo/'benchmarks/ltx-ben-immediate-a2v-v002.json').read_text())
first=json.loads((repo/'benchmarks/ltx-ben-native-a2v-v002.json').read_text())
wan=json.loads((repo/'benchmarks/local/ben-performance-state.json').read_text())
cases=[('native_mcp_rejected',native['asset']['path']),('native_immediate_unapproved',immediate['output']),('native_first_unapproved',first['output']),('musetalk_rejected',wan['lipsync']['path'])]
state.update(status='running',directory=str(directory),source_revision=source['revision'],detector=detector,limitations=['Three-second clips have fewer speaking samples than the reference control.','Raw confidence is not a probability or an acting-quality score.','All detected qualifying tracks are retained; verify the actor crop before interpreting a result.','No automatic approval, retiming, source overwrite or new video generation.'])
state_path.write_text(json.dumps(state,indent=2)+'\n')
python=str(repo/'.venv-sync-eval/bin/python')
def run(command,log):
    with log.open('w') as stream:subprocess.run(command,cwd=source_dir,env=env,stdout=stream,stderr=subprocess.STDOUT,check=True,timeout=900)
for name,path in cases:
    reference=name+'-'+uuid.uuid4().hex[:8]
    case_dir=directory/name;case_dir.mkdir()
    original=Path(path);original_hash=hashlib.sha256(original.read_bytes()).hexdigest()
    normalized=case_dir/'normalized.mkv'
    run([ffmpeg,'-v','error','-nostdin','-i',str(original),'-t','3','-vf','fps=25','-c:v','libx264','-crf','0','-preset','veryfast','-c:a','pcm_s16le','-n',str(normalized)],case_dir/'normalize.log')
    started=time.monotonic()
    run([python,str(source_dir/'run_pipeline.py'),'--videofile',str(normalized),'--reference',reference,'--data_dir',str(case_dir),'--min_track','25','--facedet_scale','0.5'],case_dir/'face-pipeline.log')
    tracks=sorted((case_dir/'pycrop'/reference).glob('0????.avi'))
    result=dict(name=name,source=str(original),source_sha256=original_hash,tracks=[])
    for index,track in enumerate(tracks):
        log=case_dir/f'track-{index}-sync.log'
        run([python,str(source_dir/'demo_syncnet.py'),'--initial_model',str(data/'models/syncnet-evaluation/syncnet_v2.model'),'--videofile',str(track),'--tmp_dir',str(case_dir/'sync-frames'),'--reference',f'track-{index}','--batch_size','8'],log)
        text=log.read_text();offset=re.search(r'AV offset:\s*(-?\d+)',text);confidence=re.search(r'Confidence:\s*([0-9.]+)',text);distance=re.search(r'Min dist:\s*([0-9.]+)',text)
        assert offset and confidence and distance,text[-2000:]
        metric=dict(track=str(track),offset_frames_at_25fps=int(offset[1]),confidence_raw=float(confidence[1]),min_embedding_distance=float(distance[1]),search_edge=abs(int(offset[1]))==15)
        result['tracks'].append(metric)
        run([ffmpeg,'-v','error','-i',str(track),'-frames:v','1','-n',str(case_dir/f'track-{index}-crop.png')],case_dir/f'track-{index}-poster.log')
    assert hashlib.sha256(original.read_bytes()).hexdigest()==original_hash
    result['wall_seconds']=time.monotonic()-started;state['results'].append(result)
    state_path.write_text(json.dumps(state,indent=2)+'\n');print(json.dumps(result),flush=True)
state['status']='scored_pending_crop_and_editorial_review';state_path.write_text(json.dumps(state,indent=2)+'\n')
print('RETAINED_SYNC_RESULTS_READY',str(state_path),flush=True)
