"""Same-input/same-converted-weight MLX vs official PyTorch audio encoder.
No text encoding, video generation, or model download. Prerequisites: pinned
source in benchmarks/local/official-ltx-audio-parity, isolated einops/safetensors
under torch-audio-parity-deps, and installed LTX/sync evaluation environments.
"""
from pathlib import Path
import sys,json,hashlib,subprocess,time,os
repo=Path(__file__).resolve().parents[2]
root=repo/'benchmarks/local/audio-encoder-parity';root.mkdir(parents=True,exist_ok=True)
source_root=repo/'benchmarks/local/official-ltx-audio-parity'
source=json.loads((source_root/'source.json').read_text())
assert source['commit']=='2362161611a61154d342e02724fb8fe58efd455d'
def sha(path):
    digest=hashlib.sha256()
    with path.open('rb') as file:
        for block in iter(lambda:file.read(1024*1024),b''):digest.update(block)
    return digest.hexdigest()
for item in source['files']:assert sha(source_root/item['path'])==item['sha256'],item['path']
if len(sys.argv)>1 and sys.argv[1]=='--torch':
    sys.path[:0]=[str(source_root),str(repo/'benchmarks/local/torch-audio-parity-deps')]
    import numpy as np,torch
    from ltx_core.model.audio_vae.audio_vae import AudioEncoder
    from ltx_core.model.common.normalization import NormType
    from ltx_core.model.audio_vae.causality_axis import CausalityAxis
    torch.set_num_threads(1)
    config=json.loads((root/'config.json').read_text())
    config['norm_type']=NormType(config['norm_type']);config['causality_axis']=CausalityAxis(config['causality_axis']);config['attn_resolutions']=set(config['attn_resolutions'])
    model=AudioEncoder(**config).eval()
    weights=np.load(root/'weights.npz')
    model.load_state_dict({name:torch.from_numpy(weights[name].copy()) for name in weights.files},strict=True)
    trace={}
    def hook(name):
        def save(module,inputs,output):trace[name]=output.detach().numpy()
        return save
    for name,module in [('conv_in',model.conv_in),('down0',model.down[0].downsample),('down1',model.down[1].downsample),('down2',model.down[2].block[-1]),('mid',model.mid.block_2),('conv_out',model.conv_out)]:module.register_forward_hook(hook(name))
    with torch.inference_mode():trace['normalized']=model(torch.from_numpy(np.load(root/'mel.npy'))).numpy()
    np.savez(root/'torch-trace.npz',**trace)
    (root/'torch-version.json').write_text(json.dumps(dict(torch=torch.__version__,device='cpu',dtype='float32',strict_weight_load=True)))
    sys.exit(0)
import fcntl,sqlite3,numpy as np
import mlx.core as mx
from ltx_pipelines_mlx import A2VidDistilledPipeline
from ltx_core_mlx.utils.audio import load_audio
from ltx_core_mlx.model.audio_vae.audio_vae import pixel_norm
app=Path.home()/'Library/Application Support/studio.DramaFactory.DramaFactory'
lock=(app/'worker.lock').open('a+');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
with sqlite3.connect(f'file:{app/"studio.sqlite"}?mode=ro',uri=True) as db:
    assert not db.execute("SELECT id FROM jobs WHERE status IN ('running','queued')").fetchall(),'Wait for the existing queue; do not overlap inference'
    config=json.loads(db.execute("SELECT value FROM settings WHERE key='worker'").fetchone()[0])
model_root=Path(config['performance_model']);audio=app/'jobs/2fb9bb13-891e-4d28-9121-9c3dcc3fd091/attempt-1/conditioned-speech.wav'
audio_hash=sha(audio);assert audio_hash=='8fd261a70de8e5d7055e55398fd3736fd6dfa8fa9e1b2023f3e87c0171d32ba9'
bin_dir=root/'bin';bin_dir.mkdir(exist_ok=True)
if not (bin_dir/'ffmpeg').exists():(bin_dir/'ffmpeg').symlink_to(config['ffmpeg'])
os.environ['PATH']=str(bin_dir)+os.pathsep+os.environ.get('PATH','');os.environ['HF_HUB_OFFLINE']='1'
mx.set_memory_limit(4_000_000_000)
pipe=A2VidDistilledPipeline(model_dir=str(model_root),gemma_model_id=config['performance_encoder'],low_ram_streaming=True)
pipe._load_audio_encoder();encoder=pipe.audio_encoder;processor=pipe.audio_processor
from mlx.utils import tree_flatten
weights={}
for key,value in tree_flatten(encoder.parameters()):
    name=key.replace('per_channel_statistics.std_of_means','per_channel_statistics.std-of-means').replace('per_channel_statistics.mean_of_means','per_channel_statistics.mean-of-means')
    tensor=np.array(value.astype(mx.float32))
    if tensor.ndim==4:tensor=tensor.transpose(0,3,1,2)
    weights[name]=tensor
np.savez(root/'weights.npz',**weights)
ddconfig=json.loads((model_root/'embedded_config.json').read_text())['audio_vae']['model']['params']['ddconfig']
(root/'config.json').write_text(json.dumps(ddconfig))
wave=load_audio(audio,target_sample_rate=16000,max_duration=73/24);assert wave is not None
mel=processor.waveform_to_mel(wave.waveform);np.save(root/'mel.npy',np.array(mel))
trace={}
def save(name,value):trace[name]=np.array(value.transpose(0,3,1,2).astype(mx.float32))
x=encoder.conv_in(mel.transpose(0,2,3,1));save('conv_in',x)
for i,block in enumerate(encoder.down):x=block(x);save(f'down{i}',x)
x=encoder.mid(x);save('mid',x)
x=encoder.conv_out(mx.sigmoid(pixel_norm(x))*pixel_norm(x));save('conv_out',x)
trace['normalized']=np.array(encoder.encode(mel).astype(mx.float32));np.savez(root/'mlx-trace.npz',**trace)
subprocess.run([str(repo/'.venv-sync-eval/bin/python'),str(Path(__file__).resolve()),'--torch'],check=True,timeout=60)
reference=np.load(root/'torch-trace.npz');metrics={}
for name,value in trace.items():
    other=reference[name];assert value.shape==other.shape,(name,value.shape,other.shape)
    assert np.isfinite(value).all() and np.isfinite(other).all()
    diff=value.astype(np.float64)-other.astype(np.float64)
    norm=np.sqrt(np.mean(other.astype(np.float64)**2))
    metrics[name]=dict(shape=list(value.shape),mean_absolute_error=float(np.abs(diff).mean()),maximum_absolute_error=float(np.abs(diff).max()),relative_rms_error=float(np.sqrt(np.mean(diff**2))/max(norm,1e-20)),cosine=float(np.dot(value.ravel().astype(np.float64),other.ravel().astype(np.float64))/(np.linalg.norm(value.astype(np.float64))*np.linalg.norm(other.astype(np.float64)))))
assert sha(audio)==audio_hash
report=dict(version='v002',official_repository=source['repository'],official_commit=source['commit'],official_source_manifest_sha256=sha(source_root/'source.json'),converted_audio_weights_sha256=sha(model_root/'audio_vae.safetensors'),conditioned_audio_sha256=audio_hash,reference_runtime=json.loads((root/'torch-version.json').read_text()),layers=metrics,peak_mlx_memory_bytes=mx.get_peak_memory(),new_video_render=False,source_preserved=True,limits='Same converted checkpoint and same input mel. Does not validate original upstream-to-MLX weight conversion against original weights, transformer audio attention, or acting/synchronization quality.')
(repo/'benchmarks/audio-encoder-parity-v002.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
