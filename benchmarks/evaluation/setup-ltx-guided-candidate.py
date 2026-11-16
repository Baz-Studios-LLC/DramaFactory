"""Pinned public dev/LoRA additions for one controlled guided A2V comparison."""
from pathlib import Path
import json,hashlib,time
from huggingface_hub import hf_hub_download
repo=Path(__file__).resolve().parents[2]
base=json.loads((repo/'benchmarks/local/ltx-candidate-download-state.json').read_text());assert base['status']=='downloaded_and_verified'
root=Path.home()/'Library/Application Support/studio.DramaFactory.DramaFactory/models/ltx-2.3-q8-guided-evaluation';root.mkdir(parents=True,exist_ok=True)
files={'transformer-dev.safetensors':(20597189549,'bd28c73e333581a81c4b3e2213051bfcce989fdb40682d22f062cf8b691455e2'),'ltx-2.3-22b-distilled-lora-384.safetensors':(7605507256,'2943ab994f3c9d88052e5a2a34cca14e4a2dfc36b1d8c407931d52d5c25dd72b')}
state={'repository':base['repository'],'revision':base['revision'],'model_root':str(root),'base_model_root':base['model_root'],'additional_bytes':sum(v[0] for v in files.values()),'status':'downloading','files':{},'hypothesis':'Compare dev+CFG/modality-guided native A2V against weak distilled synchronization; not a production switch or CFG-only causal test.'}
checkpoint=repo/'benchmarks/local/ltx-guided-download-state.json'
def save():checkpoint.write_text(json.dumps(state,indent=2)+'\n')
def sha(path):
 h=hashlib.sha256()
 with path.open('rb') as stream:
  for block in iter(lambda:stream.read(8*1024*1024),b''):h.update(block)
 return h.hexdigest()
save()
for source in Path(base['model_root']).iterdir():
 if source.is_file() and source.name not in files:
  target=root/source.name
  if not target.exists():target.symlink_to(source)
  assert target.resolve()==source.resolve(),'Preserve unexpected evaluation content'
for name,(size,expected) in files.items():
 state['current_file']=name;save()
 path=Path(hf_hub_download(base['repository'],name,revision=base['revision'],local_dir=root))
 assert path.stat().st_size==size and sha(path)==expected,name
 state['files'][name]={'bytes':size,'sha256':expected};save();print('VERIFIED',name,flush=True)
state['status']='downloaded_and_verified';save();print('GUIDED_COMPONENTS_READY',str(root),flush=True)
