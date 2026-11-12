"""Validate a real native pipeline self-test: changing video and three isolated tones."""
from array import array
import json
import math
from pathlib import Path
import statistics
import subprocess
import sys


def output(*args):
    return subprocess.check_output(args, stderr=subprocess.PIPE)


def verify(path, silent=False):
    metadata = json.loads(output('ffprobe', '-v', 'error', '-show_streams', '-of', 'json', str(path)))
    videos = [stream for stream in metadata['streams'] if stream['codec_type'] == 'video']
    audios = [stream for stream in metadata['streams'] if stream['codec_type'] == 'audio']
    assert len(videos) == 1 and len(audios) == 3, 'Expected H.264 video plus Game, Chat and Microphone'
    assert videos[0]['codec_name'] == 'h264' and videos[0]['width'] == 640 and videos[0]['height'] == 360
    assert all(stream['codec_name'] == 'aac' and stream['sample_rate'] == '48000' for stream in audios)
    durations = [float(stream['duration']) for stream in [*videos, *audios]]
    assert min(durations) >= 1.5, f'Replay duration is too short: {durations}'
    assert max(durations) - min(durations) < 0.2, f'A/V timestamps drifted: {durations}'
    video = output('ffmpeg', '-v', 'error', '-i', str(path), '-map', '0:v:0', '-vf', 'scale=1:1', '-pix_fmt', 'rgb24', '-f', 'rawvideo', '-')
    assert len(video) >= 60 * 3 and statistics.pstdev(video[::3]) > 20, 'Captured video is frozen or blank'
    frequencies = [440, 660, 880]
    for track, expected in enumerate(frequencies):
        samples = array('f')
        samples.frombytes(output('ffmpeg', '-v', 'error', '-i', str(path), '-map', f'0:a:{track}', '-t', '1', '-ac', '1', '-ar', '48000', '-f', 'f32le', '-'))
        if silent:
            assert len(samples) > 40000 and max(abs(value) for value in samples) < 0.001, 'Disabled audio captured unexpected samples'
            continue
        amplitudes = []
        for frequency in frequencies:
            angle = 2 * math.pi * frequency / 48000
            real = sum(value * math.cos(angle * index) for index, value in enumerate(samples))
            imaginary = sum(value * math.sin(angle * index) for index, value in enumerate(samples))
            amplitudes.append(2 * math.hypot(real, imaginary) / len(samples))
        assert amplitudes[track] > 0.035, f'{expected} Hz tone missing on track {track}: {amplitudes}'
        assert all(value < amplitudes[track] * 0.1 for index, value in enumerate(amplitudes) if index != track), f'Audio leaked between tracks: {amplitudes}'
    print(json.dumps({'ok': True, 'video': 'changing H.264 frames', 'audio': 'three silent AAC tracks' if silent else 'three isolated AAC tracks', 'durations': durations}))


if __name__ == '__main__':
    verify(Path(sys.argv[1]), '--silent-audio' in sys.argv[2:])
