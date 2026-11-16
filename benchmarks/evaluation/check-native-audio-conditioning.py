"""Compare installed MLX preprocessing to pinned official torch implementation.
Uses retained conditioning audio; no text encoder, video denoiser or new take.
Run with .venv-ltx-eval/bin/python; torch reference runs in .venv-sync-eval.
"""
from pathlib import Path
import json,sys,os,hashlib,subprocess
repo=Path(__file__).resolve().parents[2]
root=repo/'benchmarks/local/ltx-audio-reference'

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
source=dict(commit='2362161611a61154d342e02724fb8fe58efd455d',url='https://raw.githubusercontent.com/Lightricks/LTX-2/2362161611a61154d342e02724fb8fe58efd455d/packages/ltx-core/src/ltx_core/model/audio_vae/ops.py',sha256='a3830dacb366dc6414b42754b4b450b71117db65b140b130d8e3eb69b8668321')
root.mkdir(parents=True,exist_ok=True)
if not (root/'ops.py').exists():
    import urllib.request
    raw=urllib.request.urlopen(source['url'],timeout=30).read()
    assert hashlib.sha256(raw).hexdigest()==source['sha256']
    with (root/'ops.py').open('xb') as dest:dest.write(raw)
assert sha(root/'ops.py')==source['sha256']
if len(sys.argv)>1 and sys.argv[1]=='--torch':
    import numpy as np,torch,types,importlib.util
    from dataclasses import dataclass
    @dataclass
    class Audio:
        waveform:object
        sampling_rate:int
    # Only the upstream Audio data container is substituted. The official
    # processor itself executes unchanged, with real torchaudio operations.
    module=types.ModuleType('ltx_core.types');module.Audio=Audio
    sys.modules['ltx_core']=types.ModuleType('ltx_core');sys.modules['ltx_core.types']=module
    spec=importlib.util.spec_from_file_location('reference_audio_ops',root/'ops.py')
    reference=importlib.util.module_from_spec(spec);spec.loader.exec_module(reference)
    processor=reference.AudioProcessor(target_sample_rate=16000,mel_bins=64,mel_hop_length=160,n_fft=1024)
    wave=torch.from_numpy(np.load(root/'waveform.npy'))
    mel=processor.waveform_to_mel(Audio(wave,16000))
    np.save(root/'torch-mel.npy',mel.numpy())
    sys.exit(0)
import sqlite3,fcntl,time,wave
import numpy as np
import mlx.core as mx
os.environ['HF_HUB_OFFLINE']='1'
app=Path.home()/'Library/Application Support/studio.DramaFactory.DramaFactory'
lock=(app/'worker.lock').open('a+');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
with sqlite3.connect(f'file:{app/"studio.sqlite"}?mode=ro',uri=True) as db:
    assert not db.execute("SELECT id FROM jobs WHERE status IN ('queued','running')").fetchall()
    config=json.loads(db.execute("SELECT value FROM settings WHERE key='worker'").fetchone()[0])
bin_dir=root/'bin';bin_dir.mkdir(exist_ok=True)
if not (bin_dir/'ffmpeg').exists():(bin_dir/'ffmpeg').symlink_to(config['ffmpeg'])
os.environ['PATH']=str(bin_dir)+os.pathsep+os.environ.get('PATH','')
from ltx_core_mlx.utils.audio import load_audio
from ltx_pipelines_mlx import A2VidDistilledPipeline
from ltx_pipelines_mlx.a2vid_two_stage import encode_source_audio
from ltx_core_mlx.utils.positions import compute_audio_positions
from ltx_core_mlx.model.audio_vae.processor import AudioProcessor
path=app/'jobs/2fb9bb13-891e-4d28-9121-9c3dcc3fd091/attempt-1/conditioned-speech.wav'
before=sha(path)
assert before=='8fd261a70de8e5d7055e55398fd3736fd6dfa8fa9e1b2023f3e87c0171d32ba9'
audio=load_audio(path,target_sample_rate=16000,max_duration=73/24)
assert audio is not None
waveform=np.array(audio.waveform);np.save(root/'waveform.npy',waveform)
mel=np.array(AudioProcessor().waveform_to_mel(audio.waveform))
subprocess.run([str(repo/'.venv-sync-eval/bin/python'),str(Path(__file__).resolve()),'--torch'],check=True)
reference=np.load(root/'torch-mel.npy');assert reference.shape==mel.shape
error=np.abs(mel-reference)
mx.set_memory_limit(4_000_000_000)
pipe=A2VidDistilledPipeline(model_dir=config['performance_model'],gemma_model_id=config['performance_encoder'],low_ram_streaming=True)
start=time.monotonic()
tokens=encode_source_audio(pipe,path,num_frames=73,frame_rate=24,start_time=0,max_duration=73/24)
speech=np.array(tokens)
# The same production encoder receives silence with the exact same format/length.
with wave.open(str(path),'rb') as original:
    params=original.getparams()
silent=root/'silent.wav'
with wave.open(str(silent),'wb') as dest:
    dest.setparams(params);dest.writeframes(bytes(params.nframes*params.nchannels*params.sampwidth))
silence=np.array(encode_source_audio(pipe,silent,num_frames=73,frame_rate=24,start_time=0,max_duration=73/24))
assert np.isfinite(speech).all() and np.isfinite(silence).all()
assert speech.shape==silence.shape and np.max(np.abs(speech-silence))>0
# Exercise both real pipeline hooks, without loading a denoising transformer.
pipe._source_audio_tokens=mx.array(speech)
positions=compute_audio_positions(speech.shape[1])
for state in [pipe._stage1_audio_state(speech.shape,positions,(10,20,11),186),pipe._stage2_audio_state(mx.zeros(speech.shape),positions,(10,40,22),188,.5)]:
    assert state.frozen and np.array_equal(np.array(state.latent),speech)
    assert np.count_nonzero(np.array(state.denoise_mask.astype(mx.float32)))==0
assert sha(path)==before
report=dict(version='v002',official_source=source,source_conditioned_audio_sha256=before,waveform_shape=list(waveform.shape),mel_shape=list(mel.shape),mel_absolute_error=dict(mean=float(error.mean()),p99=float(np.quantile(error,.99)),maximum=float(error.max())),tokens_shape=list(speech.shape),speech_silence_token_rms_difference=float(np.sqrt(np.mean((speech-silence)**2))),tokens_finite=True,both_stage_hooks_preserve_frozen_speech=True,audio_encoder_seconds=time.monotonic()-start,peak_mlx_memory_bytes=mx.get_peak_memory(),new_video_inference=False,source_preserved=True,limits='Preprocessor parity and encoder sensitivity only. Does not establish full encoder numerical parity, transformer attention efficacy, intelligibility or synchronized acting.')
(repo/'benchmarks/native-audio-conditioning-v002.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
