#!/usr/bin/env python3
"""
Preprocess brooder snapshot images before uploading to Roboflow.

Deduplicates by MD5 hash, filters blurry images via Laplacian variance,
and copies passing images to an output directory.

Dependencies:
    pip install opencv-python-headless

Usage:
    python roboflow_prep.py ./snapshots ./roboflow-ready
    python roboflow_prep.py ./snapshots ./roboflow-ready --blur-threshold 80 --dry-run
"""

import argparse
import hashlib
import os
import shutil
import sys

try:
    import cv2
except ImportError:
    print("Error: opencv-python-headless is required.")
    print("  pip install opencv-python-headless")
    sys.exit(1)


def md5_file(path):
    """Compute MD5 hash of a file."""
    h = hashlib.md5()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(8192), b""):
            h.update(chunk)
    return h.hexdigest()


def laplacian_variance(path):
    """Compute Laplacian variance as a blur metric. Higher = sharper."""
    img = cv2.imread(path, cv2.IMREAD_GRAYSCALE)
    if img is None:
        return -1.0
    return cv2.Laplacian(img, cv2.CV_64F).var()


def main():
    parser = argparse.ArgumentParser(
        description="Preprocess brooder snapshots for Roboflow upload"
    )
    parser.add_argument("input_dir", help="Directory containing source JPEG images")
    parser.add_argument("output_dir", help="Directory to copy passing images into")
    parser.add_argument(
        "--blur-threshold",
        type=float,
        default=100.0,
        help="Laplacian variance threshold — images below this are considered blurry (default: 100)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print what would be copied without actually copying",
    )
    args = parser.parse_args()

    if not os.path.isdir(args.input_dir):
        print(f"Error: input directory '{args.input_dir}' does not exist")
        sys.exit(1)

    # Gather all image files
    extensions = {".jpg", ".jpeg", ".png", ".bmp", ".webp"}
    files = sorted(
        f
        for f in os.listdir(args.input_dir)
        if os.path.splitext(f)[1].lower() in extensions
    )

    total = len(files)
    duplicates = 0
    blurry = 0
    copied = 0
    seen_hashes = set()

    if not args.dry_run:
        os.makedirs(args.output_dir, exist_ok=True)

    print(f"Scanning {total} images in {args.input_dir}")
    print(f"Blur threshold: {args.blur_threshold} (Laplacian variance)")
    if args.dry_run:
        print("DRY RUN — no files will be copied\n")
    print()

    for filename in files:
        filepath = os.path.join(args.input_dir, filename)

        # Deduplicate by MD5
        file_hash = md5_file(filepath)
        if file_hash in seen_hashes:
            duplicates += 1
            print(f"  SKIP (duplicate)  {filename}")
            continue
        seen_hashes.add(file_hash)

        # Blur detection
        variance = laplacian_variance(filepath)
        if variance < 0:
            blurry += 1
            print(f"  SKIP (unreadable) {filename}")
            continue
        if variance < args.blur_threshold:
            blurry += 1
            print(f"  SKIP (blurry {variance:.0f} < {args.blur_threshold:.0f})  {filename}")
            continue

        # Copy to output
        if args.dry_run:
            print(f"  PASS (sharp {variance:.0f})  {filename}")
        else:
            dest = os.path.join(args.output_dir, filename)
            shutil.copy2(filepath, dest)
            print(f"  COPY (sharp {variance:.0f})  {filename}")
        copied += 1

    # Summary
    print()
    print("=" * 50)
    print(f"  Total images:      {total}")
    print(f"  Duplicates skipped: {duplicates}")
    print(f"  Blurry skipped:    {blurry}")
    print(f"  Images {'would copy' if args.dry_run else 'copied'}:  {copied}")
    print("=" * 50)

    if not args.dry_run and copied > 0:
        print(f"\nReady for upload: {args.output_dir}/")


if __name__ == "__main__":
    main()
