"""One-shot verification of the Pascal VOC annotation upload to Roboflow.

Uploads a single synthetic frame + a one-box ``egg`` VOC annotation to the
``incubation-stages`` project and prints exactly what was sent and returned, so
you can confirm the class reads ``egg`` (not ``0``).

Run ON THE PI (it has ROBOFLOW_API_KEY in ~/.indoor-pipeline-secrets):

    cd ~/QuailSyncV2/indoor-pipeline && set -a && . ~/.indoor-pipeline-secrets && set +a && venv/bin/python verify_voc_upload.py

It uploads to batch ``voc-verify`` (a distinct batch so it's trivial to find and
delete in the Roboflow UI) and prints the VOC XML (showing ``<name>egg</name>``)
plus the HTTP status + body of the upload and annotate calls. Final confirmation:
open Roboflow → quail/incubation-stages → batch ``voc-verify`` and check the box's
class reads ``egg``.
"""

from __future__ import annotations

import os
import sys

import numpy as np

from detector import Detection
from roboflow_uploader import INCUBATION_CLASS_NAMES, RoboflowUploader, voc_xml_annotation


def main() -> int:
    api_key = os.environ.get("ROBOFLOW_API_KEY")
    if not api_key:
        print(
            "ROBOFLOW_API_KEY not set — source the secrets first:\n"
            "  set -a && . ~/.indoor-pipeline-secrets && set +a",
            file=sys.stderr,
        )
        return 2

    # A synthetic frame + one 'egg' detection (class 0).
    frame = np.full((480, 640, 3), 128, dtype=np.uint8)
    det = Detection(class_name="egg", class_id=0, confidence=0.9, bbox=[220.0, 180.0, 420.0, 360.0])

    xml = voc_xml_annotation([det], "voc_verify.jpg", 640, 480, INCUBATION_CLASS_NAMES)
    print("=== VOC XML being uploaded ===")
    print(xml)

    import requests

    def logging_post(url, **kwargs):
        params = kwargs.get("params", {})
        tag = "IMAGE" if url.endswith("/upload") else "ANNOTATE"
        print(f"\n=== POST {tag} ===")
        print("url:", url)
        print("name:", params.get("name"), "| labelmap param present:", "labelmap" in params)
        resp = requests.post(url, **kwargs)
        print("status:", resp.status_code)
        print("body:", (getattr(resp, "text", "") or "")[:800])
        return resp

    up = RoboflowUploader(
        api_key=api_key,
        workspace="quail",
        project="incubation-stages",
        batch_name="voc-verify",
        post=logging_post,
        class_names=INCUBATION_CLASS_NAMES,
    )
    ok = up.upload_frame(frame, "voc_verify.jpg", [det])
    print("\nupload_frame ->", ok)
    print(
        "Now open Roboflow -> quail/incubation-stages -> batch 'voc-verify' and "
        "confirm the box's class reads 'egg' (not '0')."
    )
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
