#!/usr/bin/env python3
"""Golden regression comparison: vision-hub (direct-ort port) vs
yolo-device-inference (usls fork) — the correctness proof for the ported
decode path.

Prerequisites (both .nep installed & running in one NeoMind instance):
    ./build.sh --single vision-hub --skip-install --yes        # + ORT_LIB_PATH
    ./build.sh --single yolo-device-inference --skip-install --yes
    # drop both .nep into $NEOMIND_DATA_DIR/extensions/packages/, start server

Usage:
    python3 tests/golden_compare.py [--port 9375] [--user admin] [--pass X]

Pass criteria: ≥ 80% of hub detections match a same-label usls detection
with IoU > 0.5. Borderline boxes near the confidence threshold may flip
(the two paths use different image resamplers); that divergence is
expected and documented in GOLDEN.md.
"""

import argparse
import base64
import json
import pathlib
import sys
import urllib.request

FIXTURE = pathlib.Path(__file__).parent / "fixtures" / "bus.jpg"


def call(base, ext, command, args, token, timeout=300):
    req = urllib.request.Request(
        f"{base}/api/extensions/{ext}/command",
        data=json.dumps({"command": command, "args": args}).encode(),
        headers={"Content-Type": "application/json", "Authorization": "Bearer " + token},
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)["data"]


def login(base, user, password):
    req = urllib.request.Request(
        f"{base}/api/auth/login",
        data=json.dumps({"username": user, "password": password}).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)["token"]


def iou(a, b):
    ax1, ay1, ax2, ay2 = a["x"], a["y"], a["x"] + a["w"], a["y"] + a["h"]
    bx1, by1, bx2, by2 = b["x"], b["y"], b["x"] + b["w"], b["y"] + b["h"]
    iw = max(0.0, min(ax2, bx2) - max(ax1, bx1))
    ih = max(0.0, min(ay2, by2) - max(ay1, by1))
    inter = iw * ih
    union = a["w"] * a["h"] + b["w"] * b["h"] - inter
    return inter / union if union > 0 else 0.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", default="9375")
    ap.add_argument("--user", default="admin")
    ap.add_argument("--password", default="test-pass-123")
    ap.add_argument("--confidence", type=float, default=0.25)
    args = ap.parse_args()
    base = f"http://127.0.0.1:{args.port}"
    token = login(base, args.user, args.password)

    img_b64 = base64.b64encode(FIXTURE.read_bytes()).decode()

    # Warm up vision-hub (first CoreML run includes specialization compile).
    call("vision-hub" and base, "vision-hub", "analyze",
         {"image": "data:image/jpeg;base64," + img_b64,
          "tasks": [{"type": "detect", "params": {"confidence": args.confidence}}]}, token)

    hub = call(base, "vision-hub", "analyze",
               {"image": "data:image/jpeg;base64," + img_b64,
                "tasks": [{"type": "detect", "params": {"confidence": args.confidence}}]}, token)
    hub_dets = [{"label": d["label"], "confidence": d["confidence"],
                 "x": d["bbox"][0], "y": d["bbox"][1], "w": d["bbox"][2], "h": d["bbox"][3]}
                for d in hub["results"][0]["detections"]]

    usls = call(base, "yolo-device-inference", "analyze_image", {"image": img_b64}, token)
    usls_dets = [{"label": d["label"], "confidence": d["confidence"],
                  "x": d["bbox"]["x"], "y": d["bbox"]["y"],
                  "w": d["bbox"]["width"], "h": d["bbox"]["height"]}
                 for d in usls["detections"] if d.get("bbox")]

    used, matched, ious = set(), 0, []
    for v in hub_dets:
        best, bj = 0.0, None
        for j, y in enumerate(usls_dets):
            if j in used or y["label"] != v["label"]:
                continue
            s = iou(v, y)
            if s > best:
                best, bj = s, j
        if bj is not None and best > 0.5:
            used.add(bj)
            matched += 1
            ious.append(best)
            print(f"MATCH {v['label']:10s} IoU={best:.3f} conf {v['confidence']:.3f} vs {usls_dets[bj]['confidence']:.3f}")
        else:
            print(f"ONLY-IN-HUB {v['label']:10s} conf={v['confidence']:.2f}")
    for j, y in enumerate(usls_dets):
        if j not in used:
            print(f"ONLY-IN-USLS {y['label']:10s} conf={y['confidence']:.2f}")

    ratio = matched / len(hub_dets) if hub_dets else 0.0
    avg_iou = sum(ious) / len(ious) if ious else 0.0
    print(f"\n{matched}/{len(hub_dets)} matched | avg IoU={avg_iou:.3f}")
    ok = ratio >= 0.8 and avg_iou >= 0.9
    print("PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
