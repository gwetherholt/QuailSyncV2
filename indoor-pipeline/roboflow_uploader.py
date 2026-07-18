"""Roboflow frame upload with pre-annotations, over the REST upload API.

Adapted from ``incubator/roboflow_uploader.py`` (REST, ``requests`` — no heavy
SDK) but, unlike that raw-frame uploader, this one ships the detections as
*reviewable pre-labels*: after uploading the full frame it POSTs an annotation
for it.

Annotation format is **per mode**:

* **Incubation model** (project ``incubation-stages``) → **Pascal VOC XML** with
  the class **name inline** (``<object><name>egg</name></object>``). This is the
  fix for boxes landing under a class literally named ``0``: the YOLO ``.txt``
  format carries only numeric indices, and Roboflow's REST *annotate* endpoint
  does **not** honor the ``labelmap`` query param — so VOC (self-describing) is
  used instead. The names come from :data:`INCUBATION_CLASS_NAMES`.
* **Chick model** (project ``find-chicks-5``) → the legacy YOLO ``.txt`` +
  ``labelmap`` path, unchanged. (NOTE: that path has the same latent labelmap
  bug; converting it is deliberately out of scope here — tracked as follow-up.)

The target **project comes from the current mode's config** (incubation-stages
vs find-chicks-5) and the service loop sets :attr:`RoboflowUploader.project` and
:attr:`RoboflowUploader.class_names` when it swaps models — ``class_names`` set
selects the VOC path, ``None`` selects the legacy YOLO path. Workspace, API key
and batch name are shared.

Strictly opt-in and best-effort, mirroring the sibling pipelines:

* Runs only when ``roboflow.enabled`` is true AND the API key is set; an unset
  key skips uploads *silently*.
* ``requests`` is imported lazily and the HTTP POST is injectable (``post=``) for
  testing.
* Every failure is caught and logged; an upload error never propagates into the
  service loop. A failed annotation POST still leaves the image uploaded.
"""

from __future__ import annotations

import json
import logging
import xml.etree.ElementTree as ET

logger = logging.getLogger("indoorpipeline.roboflow_uploader")

# Canonical incubation class-index -> name map. SINGLE SOURCE OF TRUTH for the
# names written into the Pascal VOC annotations uploaded to the Roboflow
# `incubation-stages` project. These strings must stay in sync with the Roboflow
# project's classes and (later) the Stage-3 event types. Today the incubation
# model only emits class 0 (`egg`); add each further class as a one-line entry as
# the model starts emitting it.
INCUBATION_CLASS_NAMES: dict[int, str] = {
    0: "egg",
    # 1: "pipped",
    # 2: "zipping",
    # 3: "wet_chick",
    # 4: "dry_chick",
    # 5: "empty_shell",
}

# Roboflow REST endpoints. The workspace is implied by the API key; the project
# (dataset) id goes in the path. Upload creates the image and returns its id;
# annotate attaches a YOLO label to that image.
UPLOAD_URL = "https://api.roboflow.com/dataset/{project}/upload"
ANNOTATE_URL = "https://api.roboflow.com/dataset/{project}/annotate/{image_id}"


def _clamp_unit(value: float) -> float:
    """Clamp to the [0, 1] range YOLO normalized coordinates must live in."""
    return min(max(value, 0.0), 1.0)


def yolo_annotation(detections, image_width: int, image_height: int) -> tuple[str, dict[str, str]]:
    """Build a YOLO-format annotation string + labelmap from ``detections``.

    Each detection becomes ``class_id x_center y_center width height`` with
    coordinates normalized to the image size, using the model's native class id
    (so the labelmap is the model's own ``{id: name}``). Returns
    ``(annotation_text, labelmap)`` where ``labelmap`` maps the *string* class id
    (as it appears in the ``.txt``) to the class name — the form Roboflow's
    ``annotation_labelmap`` wants. Returns ``("", {})`` when there's nothing to
    annotate.
    """
    if image_width <= 0 or image_height <= 0:
        return "", {}
    lines: list[str] = []
    labelmap: dict[str, str] = {}
    for det in detections:
        if len(det.bbox) != 4:
            continue
        x1, y1, x2, y2 = det.bbox
        cx = _clamp_unit(((x1 + x2) / 2.0) / image_width)
        cy = _clamp_unit(((y1 + y2) / 2.0) / image_height)
        w = _clamp_unit(abs(x2 - x1) / image_width)
        h = _clamp_unit(abs(y2 - y1) / image_height)
        lines.append(f"{det.class_id} {cx:.6f} {cy:.6f} {w:.6f} {h:.6f}")
        labelmap[str(det.class_id)] = det.class_name
    text = "\n".join(lines)
    if lines:
        text += "\n"
    return text, labelmap


def _voc_class_name(det, class_names: dict[int, str]) -> str:
    """Resolve a detection's Pascal VOC ``<name>`` from the canonical map.

    Prefers ``class_names`` (the authoritative index->name mapping); falls back to
    the model's own ``class_name`` then the raw index so an as-yet-unmapped class
    still uploads with *some* name rather than crashing.
    """
    mapped = class_names.get(det.class_id)
    if mapped:
        return mapped
    return det.class_name or str(det.class_id)


def voc_xml_annotation(
    detections, filename: str, image_width: int, image_height: int, class_names: dict[int, str]
) -> str:
    """Build a Pascal VOC XML annotation with class **names inline**.

    Unlike the YOLO ``.txt`` format (numeric indices + a separate labelmap that
    Roboflow's REST annotate endpoint does not honor — boxes land under class
    ``"0"``), VOC XML embeds ``<object><name>egg</name></object>`` directly, so
    the class is self-describing and imports correctly. Bounding boxes are
    absolute pixel ``xmin/ymin/xmax/ymax`` clamped to the image. Returns ``""``
    when there is nothing valid to annotate.
    """
    if image_width <= 0 or image_height <= 0:
        return ""
    ann = ET.Element("annotation")
    ET.SubElement(ann, "filename").text = filename
    size = ET.SubElement(ann, "size")
    ET.SubElement(size, "width").text = str(int(image_width))
    ET.SubElement(size, "height").text = str(int(image_height))
    ET.SubElement(size, "depth").text = "3"

    objects = 0
    for det in detections:
        if len(det.bbox) != 4:
            continue
        x1, y1, x2, y2 = det.bbox
        xmin = max(0, min(int(round(min(x1, x2))), image_width))
        ymin = max(0, min(int(round(min(y1, y2))), image_height))
        xmax = max(0, min(int(round(max(x1, x2))), image_width))
        ymax = max(0, min(int(round(max(y1, y2))), image_height))
        if xmax <= xmin or ymax <= ymin:  # skip degenerate/out-of-frame boxes
            continue
        obj = ET.SubElement(ann, "object")
        ET.SubElement(obj, "name").text = _voc_class_name(det, class_names)
        ET.SubElement(obj, "pose").text = "Unspecified"
        ET.SubElement(obj, "truncated").text = "0"
        ET.SubElement(obj, "difficult").text = "0"
        bnd = ET.SubElement(obj, "bndbox")
        ET.SubElement(bnd, "xmin").text = str(xmin)
        ET.SubElement(bnd, "ymin").text = str(ymin)
        ET.SubElement(bnd, "xmax").text = str(xmax)
        ET.SubElement(bnd, "ymax").text = str(ymax)
        objects += 1

    if objects == 0:
        return ""
    return ET.tostring(ann, encoding="unicode")


class RoboflowUploader:
    """Uploads frames + YOLO pre-annotations to a Roboflow project (REST).

    ``project`` is mutable: the service loop points it at the current mode's
    project on a model swap. ``post`` (defaulting to ``requests.post``) and
    ``cv2_module`` are injectable so uploads are testable without the network or
    OpenCV.
    """

    def __init__(
        self,
        api_key: str,
        workspace: str,
        project: str,
        *,
        batch_name: str = "indoor-auto",
        post=None,
        timeout: float = 30.0,
        class_names: dict[int, str] | None = None,
    ):
        self.api_key = api_key
        self.workspace = workspace
        self.project = project
        self.batch_name = batch_name
        self._post = post
        self.timeout = timeout
        # When set, annotations upload as Pascal VOC XML (class names inline) using
        # this canonical index->name map — the incubation path. When None, the
        # legacy YOLO-txt + labelmap path is used — the chick path. The service
        # loop sets this per mode on a model swap.
        self.class_names = class_names

    @classmethod
    def from_config(cls, conf, project: str, *, post=None) -> "RoboflowUploader":
        """Build an uploader from a loaded :class:`config.Config` for ``project``
        (requires the key set)."""
        rf = conf.roboflow
        if not rf.api_key:
            raise RuntimeError(f"{rf.api_key_env} is not set")
        return cls(
            api_key=rf.api_key,
            workspace=rf.workspace,
            project=project,
            batch_name=rf.batch_name,
            post=post,
        )

    def _post_fn(self):
        """Resolve the HTTP POST callable, importing ``requests`` lazily."""
        if self._post is None:
            try:
                import requests  # lazy: only a real upload needs it
            except ImportError as exc:  # pragma: no cover - runtime-only path
                raise RuntimeError(
                    "requests is required for Roboflow upload (pip install requests)"
                ) from exc
            self._post = requests.post
        return self._post

    def _encode_jpeg(self, frame, cv2_module=None) -> bytes | None:
        """Encode a BGR numpy frame to JPEG bytes, or ``None`` on failure."""
        cv2 = cv2_module
        if cv2 is None:
            try:
                import cv2  # lazy: shared with the rest of the pipeline
            except ImportError as exc:  # pragma: no cover - runtime-only path
                logger.warning("opencv not available to encode frame: %s", exc)
                return None
        ok, buf = cv2.imencode(".jpg", frame)
        if not ok:
            logger.warning("Failed to JPEG-encode frame for Roboflow upload")
            return None
        return buf.tobytes()

    def _upload_image(self, data: bytes, name: str) -> str | None:
        """POST the raw JPEG, returning the created image id (or ``None``)."""
        url = UPLOAD_URL.format(project=self.project)
        params = {
            "api_key": self.api_key,
            "name": name,
            "batch_name": self.batch_name,
            "split": "train",
        }
        post = self._post_fn()
        try:
            resp = post(
                url,
                params=params,
                files={"file": (name, data, "image/jpeg")},
                timeout=self.timeout,
            )
        except Exception as exc:  # noqa: BLE001 — upload is best-effort, never fatal
            logger.warning("Roboflow upload error for %s: %s", name, exc)
            return None

        status = getattr(resp, "status_code", 200)
        if status >= 400:
            body = getattr(resp, "text", "")
            logger.warning("Roboflow upload failed (HTTP %s) for %s: %s", status, name, body)
            return None
        try:
            payload = resp.json()
        except Exception:  # noqa: BLE001 — a non-JSON 2xx still counts as uploaded
            payload = {}
        image_id = payload.get("id") if isinstance(payload, dict) else None
        logger.info(
            "Uploaded %s to Roboflow %s/%s (batch=%s, id=%s)",
            name,
            self.workspace,
            self.project,
            self.batch_name,
            image_id,
        )
        return image_id

    def _upload_annotation(
        self,
        image_id: str,
        name: str,
        body: str,
        *,
        extension: str,
        content_type: str,
        labelmap: dict[str, str] | None = None,
    ) -> bool:
        """Attach an annotation (VOC XML or YOLO txt) to an already-uploaded image.

        ``labelmap`` is included only for the YOLO path (Roboflow does not honor
        it, hence VOC for incubation); VOC needs none — names are inline."""
        url = ANNOTATE_URL.format(project=self.project, image_id=image_id)
        params = {"api_key": self.api_key, "name": f"{name}{extension}"}
        if labelmap is not None:
            params["labelmap"] = json.dumps(labelmap)
        post = self._post_fn()
        try:
            resp = post(
                url,
                params=params,
                data=body.encode("utf-8"),
                headers={"Content-Type": content_type},
                timeout=self.timeout,
            )
        except Exception as exc:  # noqa: BLE001 — annotation is best-effort
            logger.warning("Roboflow annotate error for %s: %s", name, exc)
            return False
        status = getattr(resp, "status_code", 200)
        if status >= 400:
            resp_body = getattr(resp, "text", "")
            logger.warning("Roboflow annotate failed (HTTP %s) for %s: %s", status, name, resp_body)
            return False
        logger.info("Annotated %s in Roboflow %s/%s (%s)",
                    name, self.workspace, self.project, extension)
        return True

    def upload_frame(self, frame, name: str, detections=None, *, cv2_module=None) -> bool:
        """Upload ``frame`` as ``name``, then its pre-annotations if any.

        Returns True when the image upload succeeds (annotation is best-effort on
        top). Never raises. When ``detections`` is empty the frame is uploaded
        unannotated (still useful dataset variety). Annotation format follows
        :attr:`class_names`: VOC XML (incubation) when set, else YOLO+labelmap
        (chick).
        """
        data = self._encode_jpeg(frame, cv2_module=cv2_module)
        if data is None:
            return False
        image_id = self._upload_image(data, name)
        if image_id is None:
            return False

        detections = detections or []
        if detections:
            try:
                h, w = frame.shape[0], frame.shape[1]
            except Exception:  # noqa: BLE001 — a shapeless frame just skips annotation
                logger.warning("Could not read frame dimensions for %s — skipping annotation", name)
                return True
            if self.class_names is not None:
                # Incubation model -> Pascal VOC XML with class names inline. This
                # replaces the two-step YOLO+labelmap upload, whose labelmap query
                # param Roboflow does not honor (boxes land under class "0").
                xml = voc_xml_annotation(detections, name, w, h, self.class_names)
                if xml:
                    self._upload_annotation(
                        image_id, name, xml, extension=".xml", content_type="text/xml"
                    )
            else:
                # Chick model (find-chicks-5) -> legacy YOLO-txt + labelmap.
                # Unchanged here; the same latent labelmap bug affects this path
                # (see the follow-up note) but is deliberately out of scope.
                annotation, labelmap = yolo_annotation(detections, w, h)
                if annotation:
                    self._upload_annotation(
                        image_id, name, annotation,
                        extension=".txt", content_type="text/plain", labelmap=labelmap,
                    )
        return True


def build_uploader(conf, project: str, *, post=None) -> RoboflowUploader | None:
    """Return an uploader when Roboflow upload is enabled AND keyed, else ``None``.

    A ``None`` return means "don't upload" — the two silent-skip cases:

    * ``roboflow.enabled`` false -> no-op.
    * key unset -> skipped *silently* (debug log), never an error.

    ``project`` is the initial mode's project; the service loop retargets the
    uploader on a model swap. A ``None`` result makes every upload a no-op.
    """
    rf = conf.roboflow
    if not rf.enabled:
        logger.debug("Roboflow upload disabled (roboflow.enabled is false)")
        return None
    if not rf.api_key:
        # Silent skip — enabling without a key is not worth shouting about.
        logger.debug("%s not set — skipping Roboflow upload", rf.api_key_env)
        return None
    logger.info(
        "Roboflow auto-upload enabled -> %s/%s (batch=%s, every %ss, on_detection=%s)",
        rf.workspace,
        project,
        rf.batch_name,
        rf.upload_interval_seconds,
        rf.upload_on_detection,
    )
    return RoboflowUploader.from_config(conf, project, post=post)
