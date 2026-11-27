"""Real stdio contract smoke test. No credentials, network, or production database."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

with tempfile.TemporaryDirectory(prefix='dramafactory-live-mcp-') as directory:
    env = dict(os.environ, DRAMAFACTORY_DATABASE=str(Path(directory) / 'studio.sqlite'))
    binary = Path(__file__).resolve().parents[1] / 'target/release/dramafactory'
    process = subprocess.Popen([str(binary), '--mcp'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, env=env)
    sequence = 0
    def request(method, params):
        global sequence
        sequence += 1
        process.stdin.write(json.dumps(dict(jsonrpc='2.0', id=sequence, method=method, params=params))+'\n')
        process.stdin.flush()
        response = json.loads(process.stdout.readline())
        assert response['id'] == sequence, response
        return response['result']
    def tool(tool_name, **args):
        result=request('tools/call',dict(name='dramafactory_'+tool_name,arguments=args))
        assert not result.get('isError'), result
        return json.loads(result['content'][0]['text'])
    try:
        request('initialize',dict(protocolVersion='2025-06-18',capabilities={},clientInfo=dict(name='live-control-test',version='1')))
        process.stdin.write(json.dumps(dict(jsonrpc='2.0',method='notifications/initialized'))+'\n');process.stdin.flush()
        definitions=request('tools/list',{})['tools']
        names={t['name'] for t in definitions}
        controls=next(t for t in definitions if t['name']=='dramafactory_control_app')
        assert {'read_episode_batch','apply_episode_batch'} <= set(controls['inputSchema']['properties']['action']['enum'])
        for name in ['get_live_workspace','control_app','get_control_result','get_episode_cut','save_episode_cut','get_release_draft','save_release_draft','publish_episode','save_portable_project']:
            assert 'dramafactory_'+name in names,name
        offline=request('tools/call',dict(name='dramafactory_get_live_workspace',arguments={}))
        assert offline.get('isError') and 'offline' in offline['content'][0]['text'],offline
        series=tool('create_series',name='MCP test',premise='Test')
        episode=tool('create_episode',series_id=series['id'],title='Episode 1')
        cut=tool('get_episode_cut',episode_id=episode['id'])
        assert cut['clips']==[]
        release=tool('get_release_draft',episode_id=episode['id'])
        draft=release['draft'];draft['title']='Test title'
        tool('save_release_draft',episode_id=episode['id'],draft=draft,expected=release['expected'])
        assert tool('get_release_draft',episode_id=episode['id'])['draft']['title']=='Test title'
        stale=request('tools/call',dict(name='dramafactory_save_release_draft',arguments=dict(episode_id=episode['id'],draft=draft,expected=None)))
        assert stale.get('isError'),stale
        denied=request('tools/call',dict(name='dramafactory_publish_episode',arguments=dict(episode_id=episode['id'],expected='',confirmation=False)))
        assert denied.get('isError') and 'confirmation' in denied['content'][0]['text'],denied
        print('Real stdio MCP: tool discovery, offline state, saved services, conflict and publish guard passed.')
    finally:
        process.stdin.close()
        process.wait(timeout=10)
