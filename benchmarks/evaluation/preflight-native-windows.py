"""Actual MCP preparation against episode 2, canceled under worker lock.
No inference, generated take, quality approval or asset replacement is claimed.
"""
from pathlib import Path
import fcntl,hashlib,json,sqlite3
repo=Path(__file__).resolve().parents[2]
api={'__file__':__file__}
exec((repo/'benchmarks/local/apply-episodes-04-06.py').read_text().split("ids=['dc577")[0],api)
call=api['call'];p=api['p']
data=Path.home()/'Library/Application Support/studio.DramaFactory.DramaFactory'
try:
    episode='8290815f-74db-424a-90ab-359ab5a3773f';cut=json.loads((repo/'benchmarks/other-chair-episode-02-tight-cut-v002.json').read_text())
    source=next(v for v in cut['placements'] if v['duration']>5.)
    shots=call('list_shots',episode_id=episode);targets=[next(s for s in shots if s['title'].startswith(prefix)) for prefix in ['02.07','02.09']]
    line=next(l for l in call('list_dialogue',shot_id=source['target_shot_id']) if l['id']==source['line']['id'])
    asset=next(a for a in call('list_assets') if a['id']==line['asset_id']);original=hashlib.sha256(Path(asset['path']).read_bytes()).hexdigest()
    before={a['id'] for a in call('list_assets')};results=[]
    with (data/'worker.lock').open('a+') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        created=[]
        try:
            for shot in targets:
                job=call('generate_performance',shot_id=shot['id'],audio_asset_id=asset['id'],source_line_id=line['id'])
                created.append(job['id'])
                assert job['status']=='queued' and job['attempts']==0,job
                with sqlite3.connect(data/'studio.sqlite') as db:payload=json.loads(db.execute('SELECT request FROM jobs WHERE id=?',(job['id'],)).fetchone()[0])
                assert payload['source_line']['id']==line['id'] and payload['audio_hash']==asset['hash'] and payload['frames']<=121
                call('cancel_job',job_id=job['id']);final=call('get_job_status',job_id=job['id'])
                assert final['status']=='canceled' and final['attempts']==0 and final['output_id'] is None,final
                results.append(dict(job=final,shot_title=shot['title'],frames=payload['frames'],audio_start=payload['audio_start'],audio_window=payload['audio_window'],source_line_id=line['id'],source_audio_sha256=payload['audio_hash']))
            assert results[0]['frames']==73 and abs(results[0]['audio_start']-.1)<1e-6 and results[0]['audio_window']['source_start']==0.
            assert results[1]['frames']==33 and results[1]['audio_start']==0. and abs(results[1]['audio_window']['source_start']-5.9)<1e-6
        finally:
            # Always cancel our queued jobs before releasing the inference lock,
            # including helper/assertion failures. SQL fallback is scoped only
            # to IDs created here, and only still-queued jobs.
            for job_id in created:
                try:call('cancel_job',job_id=job_id)
                except Exception:
                    with sqlite3.connect(data/'studio.sqlite') as db:
                        db.execute("UPDATE jobs SET cancel_requested=1,status='canceled',error='Preflight cleanup before inference' WHERE id=? AND status='queued'",(job_id,))
        fcntl.flock(lock,fcntl.LOCK_UN)
    assert {a['id'] for a in call('list_assets')}==before
    assert hashlib.sha256(Path(asset['path']).read_bytes()).hexdigest()==original
    report=dict(version='v002',status='real_mcp_preflight_canceled_before_inference',episode_id=episode,source_speech_seconds=asset['duration'],requests=results,scope='Actual queued first/continuation requests against the 65s edit; worker lock held, canceled at attempt zero, no new assets or model inference. Does not demonstrate acting or lip synchronization.')
    (repo/'benchmarks/native-window-preflight-v002.json').write_text(json.dumps(report,indent=2)+'\n')
    print('Actual episode-2 native preflight passed: 73-frame first window and 33-frame continuation at source offset 5.9s, canceled before inference; source and assets unchanged.')
finally:
    p.stdin.close();p.wait(timeout=10)
