"""One bounded joint video+audio generation test, not an episode or approval.
No supplied speech waveform, lip compositor, prompt enhancer or local writer.
"""
from pathlib import Path
import json,os,time,hashlib,sqlite3,subprocess,signal,fcntl,uuid,shutil
repo=Path(__file__).resolve().parents[2]
app=Path.home()/'Library/Application Support/studio.DramaFactory.DramaFactory'
source=json.loads((repo/'benchmarks/local/ben-performance-state.json').read_text())
def sha(path):
    digest=hashlib.sha256()
    with path.open('rb') as file:
        for block in iter(lambda:file.read(1024*1024),b''):digest.update(block)
    return digest.hexdigest()
reference=Path(source['reference']);assert sha(reference)==source['reference_sha256']
lock=(app/'worker.lock').open('a+');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
with sqlite3.connect(f'file:{app/"studio.sqlite"}?mode=ro',uri=True) as db:
    active=db.execute("SELECT id,status FROM jobs WHERE status IN ('running','queued')").fetchall();assert not active,active
    config=json.loads(db.execute("SELECT value FROM settings WHERE key='worker'").fetchone()[0])
directory=app/'benchmarks'/('ltx-ben-joint-'+str(uuid.uuid4()));directory.mkdir(parents=True)
local_reference=directory/'reference.png';shutil.copyfile(reference,local_reference);assert sha(local_reference)==source['reference_sha256']
bin_dir=directory/'bin';bin_dir.mkdir();(bin_dir/'ffmpeg').symlink_to(config['ffmpeg'])
os.environ['PATH']=str(bin_dir)+os.pathsep+os.environ.get('PATH','');os.environ['HF_HUB_OFFLINE']='1';os.environ['TOKENIZERS_PARALLELISM']='false'
prompt='A live-action medium portrait of the same adult 25-year-old man as the reference image, standing in the warm furniture showroom. He shifts his weight, raises one hand in a small open-palmed gesture, and says clearly in a natural young American male voice, "I need to talk to my girlfriend." His nervous sincerity shows in his eyebrows and his face as he speaks, then he lowers his hand and listens. Preserve his youthful olive face, dark curly hair, brown eyes, very light stubble, navy overshirt and ivory T-shirt. His face stays visible. Steady eye-level camera, one continuous shot, realistic skin and hands. Audio: his spoken sentence, quiet room tone, no music.'
output=directory/'joint-dialogue.mp4'
report=dict(version='v002',status='running',pid=os.getpid(),hypothesis='Jointly generating speech and picture may produce more coherent articulation than freezing an external voice track. First mode evaluation, not a duration-matched controlled ablation.',model_path=config['performance_model'],encoder_path=config['performance_encoder'],source_commit='747b30ef10e92a011e85b2aa0f6c4ee86ced5218',reference_sha256=source['reference_sha256'],reference=str(local_reference),prompt=prompt,frames=97,fps=24,width=704,height=1280,seed=186,stage1_steps=8,stage2_steps=3,output=str(output),supplied_audio=False,lip_compositing=False,notes='Single free local benchmark, no production config/selection/review changes. Exact approved Ben reference retained. Generated voice identity/transcript and acting require review.')
state=repo/'benchmarks/local/ltx-ben-joint-state.json'
def save():
    raw=json.dumps(report,indent=2)+'\n';state.write_text(raw);(directory/'benchmark.json').write_text(raw)
save()
def expire(*_):raise TimeoutError('Joint generation exceeded bounded 30-minute evaluation')
signal.signal(signal.SIGALRM,expire);signal.alarm(1800)
start=time.monotonic()
import mlx.core as mx
mx.set_memory_limit(40_000_000_000);mx.set_cache_limit(1_000_000_000)
try:
    from ltx_pipelines_mlx import DistilledPipeline
    pipe=DistilledPipeline(model_dir=config['performance_model'],gemma_model_id=config['performance_encoder'],low_ram_streaming=True)
    pipe.generate_and_save(prompt=prompt,output_path=str(output),image=str(local_reference),num_frames=97,height=1280,width=704,frame_rate=24.,seed=186,stage1_steps=8,stage2_steps=3)
    subprocess.run([config['ffmpeg'],'-v','error','-xerror','-i',str(output),'-f','null','-'],check=True,timeout=60)
    for frame in [0,12,24,36,48,60,72,84,96]:
        subprocess.run([config['ffmpeg'],'-v','error','-i',str(output),'-vf',f"select='eq(n,{frame})'",'-frames:v','1','-n',str(directory/f'frame-{frame:03d}.png')],check=True,timeout=30)
    subprocess.run([config['ffmpeg'],'-v','error','-i',str(output),'-vn','-ac','1','-ar','16000','-c:a','pcm_s16le','-n',str(directory/'generated-speech.wav')],check=True,timeout=30)
    assert sha(reference)==sha(local_reference)==source['reference_sha256']
    report.update(status='awaiting_actual_review',output_sha256=sha(output),generated_audio_sha256=sha(directory/'generated-speech.wav'))
except BaseException as error:
    report.update(status='failed',error=repr(error));raise
finally:
    signal.alarm(0);report.update(wall_seconds=time.monotonic()-start,peak_mlx_memory_bytes=mx.get_peak_memory());save();print('JOINT_DIALOGUE_RESULT',json.dumps(report),flush=True)
