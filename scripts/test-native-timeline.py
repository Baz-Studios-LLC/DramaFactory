#!/usr/bin/env python3
"""Real AVFoundation source playback test; disposable generated media, no hosted calls."""
import json
import os
import pathlib
import queue
import struct
import subprocess
import tempfile
import threading
import time

ffmpeg = os.environ['DRAMAFACTORY_TEST_FFMPEG']
root = pathlib.Path(__file__).resolve().parent.parent
with tempfile.TemporaryDirectory(prefix='df-native-test-') as folder:
    folder = pathlib.Path(folder)
    binary = folder / 'player'
    subprocess.run(['xcrun', 'swiftc', '-O', '-suppress-warnings', str(root/'native/TimelinePlayer.swift'), '-o', str(binary)], check=True)
    for color in ['red', 'blue']:
        subprocess.run([ffmpeg, '-v', 'error', '-f', 'lavfi', '-i', f'color={color}:s=320x180:r=24:d=4,drawbox=x=0:y=0:w=320:h=180:color=yellow:t=fill:enable=lt(t\\,0.8),drawbox=x=0:y=0:w=320:h=30:color=lime:t=fill', '-f', 'lavfi', '-i', 'sine=frequency=440:duration=4', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-shortest', str(folder/f'{color}.mp4')], check=True)
    subprocess.run([ffmpeg, '-v', 'error', '-f', 'lavfi', '-i', 'sine=frequency=220:duration=8', str(folder/'music.wav')], check=True)
    manifest = {'clips': [{'path': str(folder/f'{color}.mp4'), 'start': 1, 'duration': 2.123456789, 'gap_before': gap, 'audio_gain_db': -60, 'audio_muted': True, 'audio_fade_in': 0.1, 'audio_fade_out': 0.1} for color, gap in [('red',0),('blue',0.512345678)]], 'cues': [{'path':str(folder/'music.wav'), 'start':0, 'source_start':1, 'duration':4.5, 'gain_db':-60, 'muted':True, 'fade_in':0.25, 'fade_out':0.25}], 'width':320, 'height':180, 'origin':0}
    process = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    packets = queue.Queue()
    def read_exact(count):
        data = bytearray()
        while len(data) < count:
            chunk = process.stdout.read(count - len(data))
            if not chunk: raise EOFError('native helper closed')
            data.extend(chunk)
        return bytes(data)
    def read_packets():
        try:
            while True:
                tag, size, width, height, position = struct.unpack('<IIIId', read_exact(24))
                payload = read_exact(size)
                if tag != 4: packets.put((tag,width,height,position,payload))
        except Exception as error: packets.put(error)
    threading.Thread(target=read_packets, daemon=True).start()
    def send(command, **kwargs):
        process.stdin.write((json.dumps({'command':command, **kwargs})+'\n').encode()); process.stdin.flush()
    def receive(timeout=12):
        p=packets.get(timeout=timeout)
        while isinstance(p, tuple) and p[0] == 4:
            p=packets.get(timeout=timeout)
        if isinstance(p, Exception): raise p
        assert p[0] != 3, p[4].decode()
        return p
    try:
        started=time.monotonic(); send('load', manifest=manifest)
        first=receive(); assert first[0]==1
        latency=time.monotonic()-started
        top=(10*first[1]+10)*4
        assert first[4][top+1] > 150 and first[4][top+1] > first[4][top]*2, 'preview was vertically flipped'
        assert latency < 8, latency
        center=((first[2]//2)*first[1]+first[1]//2)*4
        assert first[4][center] > 150 and first[4][center+1] < 80, 'source in-point was ignored'
        colors=set()
        def sample(p):
            if p[0] == 1:
                pixels=p[4]; i=((p[2]//2)*p[1]+p[1]//2)*4
                r,g,b=pixels[i:i+3]
                if r>150 and b<50: colors.add('red')
                elif b>150 and r<50: colors.add('blue')
                elif max(r,g,b)<20: colors.add('gap')
        sample(first)
        send('pause'); time.sleep(0.2)
        while not packets.empty(): sample(receive())
        assert packets.empty()
        time.sleep(0.25)
        assert packets.empty(), 'frames advanced while paused'
        send('play')
        updated=False
        while True:
            p=receive(); sample(p)
            if p[3]>0.5 and not updated:
                manifest['cues'][0]['gain_db']=-30
                send('update', manifest=manifest); updated=True
            if p[0]==2: break
        assert colors=={'red','blue','gap'}, colors
        manifest['origin']=3.0
        send('load', manifest=manifest); send('play')
        p=receive(); assert p[0]==1 and p[3]>=2.99, p[:4]
        print(json.dumps({'first_frame_seconds':round(latency,3), 'checks':['fractional source trims','cut boundary','black gap','music mix construction','pause/resume','live mix update','seek'], 'audio_listening_verified':False}))
    finally:
        process.terminate()
        try: process.wait(timeout=3)
        except subprocess.TimeoutExpired: process.kill(); process.wait()
