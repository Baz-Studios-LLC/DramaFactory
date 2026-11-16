"""Read-only real packaged MCP environment checks. No inference/downloads."""
from pathlib import Path
import json,subprocess,select,time
repo=Path(__file__).resolve().parents[2]
binary=repo/'target/DramaFactory.app/Contents/MacOS/dramafactory'
p=subprocess.Popen([str(binary),'--mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
seq=0
def rpc(method,params):
    global seq
    seq+=1;p.stdin.write(json.dumps(dict(jsonrpc='2.0',id=seq,method=method,params=params))+'\n');p.stdin.flush()
    assert select.select([p.stdout],[],[],40)[0],method
    r=json.loads(p.stdout.readline());assert r['id']==seq;return r['result']
def call(tool,**args):
    r=rpc('tools/call',dict(name='dramafactory_'+tool,arguments=args));assert not r.get('isError'),r
    return json.loads(r['content'][0]['text'])
try:
    rpc('initialize',dict(protocolVersion='2025-06-18',capabilities={},clientInfo=dict(name='Installed runtime inspection',version='v002')))
    p.stdin.write(json.dumps(dict(jsonrpc='2.0',method='notifications/initialized'))+'\n');p.stdin.flush()
    reports=[]
    for model in call('list_available_models'):
        report=call('check_model_runtime',model_id=model['id']);reports.append(report)
        print(json.dumps(dict(model=model['id'],passed=report['passed'],seconds=report['elapsed_seconds'],errors=report['errors'])),flush=True)
    (repo/'benchmarks/installed-runtimes-v002.json').write_text(json.dumps(dict(version='v002',reports=reports,limits='Import/executable snapshot only; no checkpoint validation, numerical parity, generation or acting acceptance. Installation/provisioning for a fresh machine remains incomplete.'),indent=2)+'\n')
finally:
    p.stdin.close();p.wait(timeout=15)
