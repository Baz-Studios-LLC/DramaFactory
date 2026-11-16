"""One bounded native image+audio-conditioned human test, not a production episode.
Only the assistant-authored visual prompt is embedded. No prompt enhancement.
"""
from pathlib import Path
import json,os,time,hashlib,sqlite3,subprocess,signal,fcntl,uuid,sys
repo=Path(__file__).resolve().parents[2]
model=json.loads((repo/'benchmarks/local/ltx-guided-download-state.json').read_text())
encoder=json.loads((repo/'benchmarks/local/ltx-encoder-download-state.json').read_text())
assert model['status']==encoder['status']=='downloaded_and_verified'
app_data=Path.home()/'Library/Application Support/studio.DramaFactory.DramaFactory'
source=json.loads((repo/'benchmarks/local/ben-performance-state.json').read_text())
reference=Path(source['reference']);audio=Path(source['speech']['path'])
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
assert sha(reference)==source['reference_sha256']
assert sha(audio)==source['speech']['hash']
lock=(app_data/'worker.lock').open('a+')
fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
directory=app_data/'benchmarks'/('ltx-ben-'+str(uuid.uuid4()));directory.mkdir(parents=True)
with sqlite3.connect(app_data/'studio.sqlite') as db:
    config=json.loads(db.execute("SELECT value FROM settings WHERE key='worker'").fetchone()[0])
ffmpeg=Path(config['ffmpeg']);assert ffmpeg.is_file()
tools=directory/'bin';tools.mkdir();(tools/'ffmpeg').symlink_to(ffmpeg)
os.environ['PATH']=str(tools)+os.pathsep+os.environ.get('PATH','')
os.environ['HF_HUB_OFFLINE']='1'
os.environ['TOKENIZERS_PARALLELISM']='false'
frames=73;fps=24;duration=frames/fps
padded=directory/'source-dialogue-padded.wav'
subprocess.run([str(ffmpeg),'-v','error','-nostdin','-i',str(audio),'-af','apad','-t',str(duration),'-ar','48000','-ac','2','-c:a','pcm_s16le','-n',str(padded)],check=True)
prompt='Live-action cinematic medium portrait of the exact same adult 25-year-old man shown in the reference image, in the same warm furniture showroom. Preserve his youthful face, brown eyes, dark curly hair, olive skin, very light stubble, navy overshirt and ivory crew-neck shirt. His warm smile fades into nervous sincerity. He shifts his shoulders, makes a small hesitant palm-up gesture, and speaks the exact recorded sentence: "I need to talk to my girlfriend." Natural clear lip articulation matching the supplied male voice. He looks just off camera toward his girlfriend and keeps his face visible, with a restrained honest expression and natural breathing. Steady eye-level camera, no cuts, photorealistic skin detail and realistic hands.'
if True:
    prompt='Live-action dialogue performance. The adult 25-year-old man in the supplied reference image is already speaking at the very beginning of the clip. Throughout the exact supplied audio he clearly articulates: "I need to talk to my girlfriend." Every word has a distinct natural mouth shape, synchronized to the recorded male voice, starting immediately without a pause or a preliminary smile. During speech his eyebrows and shoulders respond naturally; a small palm-up hand gesture accompanies his concern. After the last word he closes his mouth and listens. Preserve the reference man exactly: youthful olive face, brown eyes, dark curly hair, very light stubble, navy overshirt, ivory T-shirt and tan trousers. Same warm furniture showroom. Photorealistic face and hands. Steady medium portrait, face unobstructed, looking toward his girlfriend just beside the camera, no cuts.'
report=dict(version='v002',kind='explicit_backend_quality_benchmark',status='running',output_directory=str(directory),source_reference=str(reference),source_reference_sha256=sha(reference),source_audio=str(audio),source_audio_sha256=sha(audio),model_repository=model['repository'],model_revision=model['revision'],encoder_repository=encoder['repo'],encoder_revision=encoder['revision'],source_commit='747b30ef10e92a011e85b2aa0f6c4ee86ced5218',prompt=prompt,width=704,height=1280,frames=frames,fps=fps,seed=186,stage1_steps=30,stage2_steps=3,cfg_scale=3.0,stg_scale=1.0,negative_prompt_setting='upstream default',low_ram_streaming=True,memory_limit_bytes=40_000_000_000,notes='One guided backend comparison, same approved Ben image, voice, prompt, seed, dimensions and frames as immediate-speech distilled baseline. No prompt enhancement or production configuration switch. Different transformer/schedule/guidance prevent attributing effects to CFG alone.')
report.update(hypothesis='Does the dev+CFG/modality-guided A2V path follow speech more reliably than the distilled path? Backend/schedule comparison, not a CFG-only ablation.',baseline='benchmarks/ltx-ben-immediate-a2v-v002.json',production_configuration_changed=False)
checkpoint=repo/'benchmarks/local/ltx-ben-guided-state.json'
checkpoint.write_text(json.dumps(report,indent=2)+'\n')
print('LTX_BEN_STARTED',str(directory),flush=True)
def expired(*_):raise TimeoutError('Bounded 30-minute candidate benchmark exceeded')
signal.signal(signal.SIGALRM,expired);signal.alarm(1800)
start=time.monotonic()
import mlx.core as mx
mx.set_memory_limit(report['memory_limit_bytes']);mx.set_cache_limit(1_000_000_000)
try:
    from ltx_pipelines_mlx import A2VidPipelineTwoStage
    pipe=A2VidPipelineTwoStage(model_dir=model['model_root'],gemma_model_id=encoder['model_root'],low_ram_streaming=True)
    output=directory/'candidate.mp4'
    pipe.generate_and_save(prompt=prompt,output_path=str(output),audio_path=str(padded),height=1280,width=704,num_frames=frames,frame_rate=24.,seed=186,stage1_steps=30,stage2_steps=3,cfg_scale=3.0,stg_scale=1.0,image=str(reference))
    subprocess.run([str(ffmpeg),'-v','error','-xerror','-i',str(output),'-f','null','-'],check=True)
    for frame in [0,36,72]:
        subprocess.run([str(ffmpeg),'-v','error','-i',str(output),'-vf',f"select='eq(n,{frame})'",'-frames:v','1','-n',str(directory/f'frame-{frame:03d}.png')],check=True)
    report.update(status='awaiting_visual_review',output=str(output),output_sha256=sha(output))
except Exception as error:
    report.update(status='failed',error=repr(error))
    raise
finally:
    signal.alarm(0)
    report.update(wall_seconds=time.monotonic()-start,peak_memory_bytes=mx.get_peak_memory())
    checkpoint.write_text(json.dumps(report,indent=2)+'\n')
    (directory/'benchmark.json').write_text(json.dumps(report,indent=2)+'\n')
    print('LTX_BEN_RESULT',json.dumps(report),flush=True)
