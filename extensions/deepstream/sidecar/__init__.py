"""DeepStream Python sidecar package.

Entry point: ``python3 -m deepstream.sidecar.deepstream_runner``
(or directly: ``python3 extensions/deepstream/sidecar/deepstream_runner.py``).

All modules are designed to import cleanly inside the
``nvcr.io/nvidia/deepstream:7.1-pyds` container (Python 3.10 + pyds
1.2.0 + DeepStream 7.1).

``protocol`` and ``config`` import on ANY Python 3.10+ host (stdlib
only) — used for unit tests on macOS.
"""

__version__ = "0.1.0"
