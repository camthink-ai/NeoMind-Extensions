"""In-proc ASR/TTS backend tests with mocked sherpa_onnx.

sherpa_onnx is ~700MB with native binaries; we mock it via sys.modules so
unit tests run without the heavy dep. Real integration is exercised
manually via the allinone profile.
"""
from __future__ import annotations

import sys
from pathlib import Path
from unittest.mock import MagicMock

import numpy as np
import pytest


# ---------------------------------------------------------------------------
# Fake sherpa_onnx module — installed into sys.modules per-test via fixture.
# ---------------------------------------------------------------------------
class _FakeStream:
    def __init__(self):
        self.result = MagicMock(text="你好世界")
        self.accepted = None

    def accept_waveform(self, sr, audio):
        self.accepted = (sr, audio)


class _FakeRecognizer:
    def __init__(self):
        self.last_stream = None

    @classmethod
    def from_sense_voice(cls, **kwargs):
        return cls()

    def create_stream(self):
        self.last_stream = _FakeStream()
        return self.last_stream

    def decode_stream(self, s):
        pass


class _FakeGeneratedAudio:
    def __init__(self, samples, sample_rate):
        self.samples = samples
        self.sample_rate = sample_rate


class _FakeOfflineTts:
    def __init__(self, cfg):
        self.cfg = cfg
        self.last_call = None

    def generate(self, text, prompt_text, prompt_samples, sample_rate):
        self.last_call = dict(text=text, prompt_text=prompt_text,
                              n_prompt=len(prompt_samples), sample_rate=sample_rate)
        # 1 second of 24kHz sine
        n = 24000
        return _FakeGeneratedAudio(
            samples=(np.sin(np.linspace(0, 440 * 2 * np.pi, n))
                     * 0.5).astype("float32"),
            sample_rate=24000,
        )


class _FakeZipvoiceModelConfig:
    def __init__(self, **kw):
        self.kw = kw


class _FakeTtsModelConfig:
    def __init__(self, zipvoice=None, num_threads=2, debug=False, provider="cpu"):
        self.zipvoice = zipvoice
        self.num_threads = num_threads


class _FakeTtsConfig:
    def __init__(self, model=None, max_num_sentences=2):
        self.model = model
        self.max_num_sentences = max_num_sentences

    def validate(self):
        return True


def _build_fake_sherpa():
    fake = MagicMock()
    fake.OfflineRecognizer = _FakeRecognizer
    fake.OfflineTts = _FakeOfflineTts
    fake.OfflineTtsZipvoiceModelConfig = _FakeZipvoiceModelConfig
    fake.OfflineTtsModelConfig = _FakeTtsModelConfig
    fake.OfflineTtsConfig = _FakeTtsConfig
    return fake


@pytest.fixture(autouse=True)
def mock_sherpa(monkeypatch):
    """Inject a fake sherpa_onnx into sys.modules and reset the cached import
    in backends.asr / backends.tts so they pick up the fake.
    """
    fake = _build_fake_sherpa()
    monkeypatch.setitem(sys.modules, "sherpa_onnx", fake)
    # Reset cached module references.
    import importlib
    import backends.asr as asr_mod
    import backends.tts as tts_mod
    asr_mod._sherpa = fake
    tts_mod._sherpa = fake
    yield fake
    asr_mod._sherpa = None
    tts_mod._sherpa = None


# ---------------------------------------------------------------------------
# ASR tests
# ---------------------------------------------------------------------------
@pytest.mark.asyncio
async def test_sensevoice_inproc_transcribe_happy_path(tmp_path, monkeypatch):
    """Plain 16kHz path — no resampling required."""
    from backends.asr import SenseVoiceInprocASR, _download_sensevoice

    # Skip the real download by ensuring model file "exists".
    monkeypatch.setattr(
        "backends.asr._download_sensevoice", lambda dest: None
    )
    # Stub Path.exists so the constructor's existence check passes without
    # creating files. We patch the specific attribute lookup via monkeypatch
    # on the module's Path reference is not trivial; instead pre-create the
    # expected files in tmp_path.
    model_subdir = tmp_path / "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17"
    model_subdir.mkdir(parents=True)
    (model_subdir / "model.int8.onnx").write_bytes(b"fake")
    (model_subdir / "tokens.txt").write_text("x")

    asr = SenseVoiceInprocASR(language="auto", model_dir=str(tmp_path))
    audio = [0.1, -0.2, 0.3] * 100
    result = await asr.transcribe(audio, 16000)
    assert result == "你好世界"
    # Stream got the audio at 16kHz
    rec = asr._recognizer
    assert isinstance(rec, _FakeRecognizer)
    assert rec.last_stream.accepted[0] == 16000
    assert len(rec.last_stream.accepted[1]) == len(audio)


@pytest.mark.asyncio
async def test_sensevoice_inproc_transcribe_resamples(tmp_path):
    """Input at 48kHz should be linearly downsampled to 16kHz."""
    from backends.asr import SenseVoiceInprocASR

    model_subdir = tmp_path / "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17"
    model_subdir.mkdir(parents=True)
    (model_subdir / "model.int8.onnx").write_bytes(b"fake")
    (model_subdir / "tokens.txt").write_text("x")

    asr = SenseVoiceInprocASR(model_dir=str(tmp_path))
    # 1s of 48kHz audio
    audio_48k = np.linspace(-0.5, 0.5, 48000).tolist()
    await asr.transcribe(audio_48k, 48000)
    rec = asr._recognizer
    # Accepted audio should be ~16000 samples (16kHz after resample)
    sr, samples = rec.last_stream.accepted
    assert sr == 16000
    assert abs(len(samples) - 16000) <= 2  # rounding tolerance


# ---------------------------------------------------------------------------
# TTS tests
# ---------------------------------------------------------------------------
@pytest.mark.asyncio
async def test_zipvoice_inproc_synthesize(tmp_path):
    """synthesize() returns int16 LE mono PCM bytes."""
    from backends.tts import ZipVoiceInprocTTS

    # Stub model + vocoder files so downloads are skipped.
    model_subdir = tmp_path / "sherpa-onnx-zipvoice-distill-int8-zh-en-emilia"
    model_subdir.mkdir(parents=True)
    for fn in ("encoder.int8.onnx", "decoder.int8.onnx", "tokens.txt",
               "lexicon.txt"):
        (model_subdir / fn).write_bytes(b"fake")
    (model_subdir / "espeak-ng-data").mkdir()
    (tmp_path / "vocos_24khz.onnx").write_bytes(b"fake")

    # Stub a prompt audio + text so _load_prompt doesn't need real soundfile.
    import soundfile as sf  # noqa: F401  (ensure import works)

    # Create a tiny wav via soundfile so _load_prompt works.
    prompt_wav = tmp_path / "prompt.wav"
    sf.write(prompt_wav, np.zeros(16000, dtype="float32"), 16000)
    prompt_text = "你好"

    tts = ZipVoiceInprocTTS(
        voice="中文女",
        model_dir=str(tmp_path),
        prompt_wav=str(prompt_wav),
        prompt_text=prompt_text,
    )

    pcm = await tts.synthesize("测试", "中文女")
    assert isinstance(pcm, bytes)
    # 1s of 24kHz int16 = 24000 * 2 bytes
    assert len(pcm) == 24000 * 2
    # Should be valid int16 LE: cast back succeeds
    arr = np.frombuffer(pcm, dtype="<i2")
    assert arr.shape == (24000,)
    # sine wave scaled by 0.5 -> non-zero, within int16 range
    assert arr.max() > 0


@pytest.mark.asyncio
async def test_zipvoice_inproc_stream_yields_single_final_chunk(tmp_path):
    """stream() yields exactly one TtsChunk with is_final=True."""
    from backends.tts import ZipVoiceInprocTTS

    model_subdir = tmp_path / "sherpa-onnx-zipvoice-distill-int8-zh-en-emilia"
    model_subdir.mkdir(parents=True)
    for fn in ("encoder.int8.onnx", "decoder.int8.onnx", "tokens.txt",
               "lexicon.txt"):
        (model_subdir / fn).write_bytes(b"fake")
    (model_subdir / "espeak-ng-data").mkdir()
    (tmp_path / "vocos_24khz.onnx").write_bytes(b"fake")

    import soundfile as sf
    prompt_wav = tmp_path / "prompt.wav"
    sf.write(prompt_wav, np.zeros(16000, dtype="float32"), 16000)

    tts = ZipVoiceInprocTTS(
        model_dir=str(tmp_path),
        prompt_wav=str(prompt_wav),
        prompt_text="你好",
    )

    chunks = []
    async for chunk in tts.stream("测试", "中文女"):
        chunks.append(chunk)
    assert len(chunks) == 1
    assert chunks[0].is_final is True
    assert chunks[0].sample_rate == 24000
    assert len(chunks[0].pcm_int16) > 0
