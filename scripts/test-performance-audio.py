"""Verify the real adapter's bounded conditioning WAV with FFmpeg; no inference.
Synthetic stereo PCM is an explicit disposable fixture, never an acting benchmark.
"""
from pathlib import Path
import array,hashlib,json,sqlite3,subprocess,sys,tempfile,wave
repo=Path(__file__).resolve().parents[1]
root=Path.home()/'Library/Application Support/studio.DramaFactory.DramaFactory'
with sqlite3.connect(root/'studio.sqlite') as db:ffmpeg=json.loads(db.execute("SELECT value FROM settings WHERE key='worker'").fetchone()[0])['ffmpeg']
with tempfile.TemporaryDirectory(prefix='df-performance-audio-') as temporary:
    directory=Path(temporary);source=directory/'six-seconds.wav'
    samples=array.array('h')
    for index in range(6*48000):
        value=1000 if index<120000 else 4000 if index<264000 else -3000
        samples.extend([value,-value])
    with wave.open(str(source),'wb') as output:output.setnchannels(2);output.setsampwidth(2);output.setframerate(48000);output.writeframes(samples.tobytes())
    original_hash=hashlib.sha256(source.read_bytes()).hexdigest()
    cases=[('first',.5,0.,[(.25,0),(.75,1000),(3.02,4000)]),('continuation',0.,2.5,[(0.,4000),(2.75,4000),(3.02,-3000)]),('late_start',2.9,0.,[(2.8,0),(3.,1000)]),('legacy',.2,None,[(.1,0),(.3,1000)])]
    for name,delay,offset,checks in cases:
        attempt=directory/name;attempt.mkdir();request=dict(frames=73,audio=str(source),audio_start=delay,setup=dict(ffmpeg=ffmpeg),output=str(attempt/'unused-video.mp4'))
        if offset is not None:request['audio_window']=dict(source_start=offset)
        payload=attempt/'request.json';payload.write_text(json.dumps(request))
        result=subprocess.run([sys.executable,str(repo/'workers/performance.py'),str(payload),'--prepare-audio-only'],check=True,capture_output=True,text=True,timeout=30)
        assert 'no inference performed' in result.stdout
        event=json.loads(result.stdout.strip());assert event['conditioned_audio_frames']==146000 and event['conditioned_audio_rms']>0 and event['conditioned_audio_peak']>0
        assert event['conditioned_audio_sha256']==hashlib.sha256((attempt/'conditioned-speech.wav').read_bytes()).hexdigest()
        with wave.open(str(attempt/'conditioned-speech.wav'),'rb') as wav:
            assert wav.getnchannels()==2 and wav.getframerate()==48000 and wav.getnframes()==146000
            actual=array.array('h');actual.frombytes(wav.readframes(wav.getnframes()))
        for second,expected in checks:
            sample=round(second*48000)*2
            assert list(actual[sample:sample+2])==[expected,-expected],(name,second,list(actual[sample:sample+2]),expected)
        assert not (attempt/'unused-video.mp4').exists()
    # An offset beyond the source produces digital silence; fail before MLX
    # import even when preparation is invoked with ordinary system Python.
    attempt=directory/'silent-window';attempt.mkdir()
    request=dict(frames=73,audio=str(source),audio_start=0.,audio_window=dict(source_start=10.),setup=dict(ffmpeg=ffmpeg),output=str(attempt/'unused-video.mp4'))
    payload=attempt/'request.json';payload.write_text(json.dumps(request))
    result=subprocess.run([sys.executable,str(repo/'workers/performance.py'),str(payload)],capture_output=True,text=True,timeout=30)
    assert result.returncode!=0 and 'digitally silent' in result.stderr and 'mlx' not in result.stderr,result.stderr
    assert (attempt/'conditioned-speech.wav').is_file() and not (attempt/'unused-video.mp4').exists()
    audit=json.loads((attempt/'conditioned-audio-audit.json').read_text())
    assert audit['conditioned_audio_rms']==0 and audit['conditioned_audio_peak']==0 and audit['conditioned_audio_rms_dbfs'] is None
    assert hashlib.sha256(source.read_bytes()).hexdigest()==original_hash
    print('Performance audio windows passed: actual adapter/FFmpeg exact stereo samples for offset, continuation, late onset and legacy requests; 73-frame length, original source preserved, digital silence rejected before MLX, no inference.')
