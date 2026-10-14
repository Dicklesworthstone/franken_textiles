"""Synthetic known-answer detection tests; no wearer images or candidate-generated oracle.

Run: python tools/photo/test_charuco_detector.py
The board renderer is OpenCV's reference renderer, independent of ftx's sheet generator.
Full generated-sheet -> Rust scale -> measurement tests live in the CLI test target.
"""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import unittest

import cv2
import numpy as np

SOURCE = Path(__file__).resolve().parents[2] / "crates/ftx-cli/src/measure/charuco_detector.py"
spec = importlib.util.spec_from_file_location("charuco_detector", SOURCE)
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)


def scene(sy=6, legacy=False, dictionary=None):
    dictionary = dictionary if dictionary is not None else cv2.aruco.DICT_4X4_50
    board = cv2.aruco.CharucoBoard((8, sy), 22, 16,
                                 cv2.aruco.getPredefinedDictionary(dictionary))
    board.setLegacyPattern(legacy)
    patch = board.generateImage((800, sy * 100))
    image = np.full((sy * 100 + 160, 980), 80, np.uint8)
    image[80:80 + sy * 100, 90:890] = patch
    header = {"schema": "ftx-charuco-request/1", "image_size_px": [980, image.shape[0]],
              "sheet": {"schema": "fabric-calib-sheet/1", "paper": "letter" if sy == 6 else "a4",
                        "board": {"type": "charuco", "dictionary": "DICT_4X4_50",
                                  "squares_x": 8, "squares_y": sy, "square_mm": 22,
                                  "marker_mm": 16, "legacy_pattern": legacy,
                                  "origin_mm": [11.43, 7.959], "size_mm": [176, sy * 22]}}}
    # OpenCV image coordinates index pixel centres; internal chessboard edges sit at k*100 - 0.5.
    truth = board.getChessboardCorners()[:, :2].astype(np.float64) * (100 / 22)
    truth += [89.5, 79.5]
    return header, image, truth


def detect(header, image):
    header = dict(header, image_size_px=[image.shape[1], image.shape[0]])
    bgr = cv2.cvtColor(image, cv2.COLOR_GRAY2BGR)
    return adapter.detect(header, bgr.tobytes())


class CharucoDetection(unittest.TestCase):
    def assert_corners(self, result, truth, minimum, tolerance=0.7):
        reference = result["reference"]
        corners = reference["corners"]
        self.assertGreaterEqual(len(corners), minimum)
        self.assertEqual([p["id"] for p in corners], sorted({p["id"] for p in corners}))
        errors = [np.linalg.norm(np.array(p["point_px"]) - truth[p["id"]]) for p in corners]
        self.assertLess(max(errors), tolerance, errors)
        self.assertNotIn("homography_px_to_board_mm", reference)
        self.assertEqual(reference["board"]["origin_mm"], [11.43, 7.959])

    def test_letter_a4_and_even_row_legacy_corner_identity(self):
        for sy, legacy in [(6, False), (7, False), (6, True)]:
            with self.subTest(sy=sy, legacy=legacy):
                header, image, truth = scene(sy, legacy)
                result = detect(header, image)
                self.assert_corners(result, truth, 7 * (sy - 1), 0.05)
                self.assertEqual(result, detect(header, image))

    def test_quarter_turns_preserve_physical_ids(self):
        header, image, truth = scene()
        for _ in range(3):
            w = image.shape[1]
            image = np.rot90(image).copy()
            truth = np.column_stack((truth[:, 1], w - 1 - truth[:, 0]))
            self.assert_corners(detect(header, image), truth, 35, 0.05)

    def test_projective_warp_noise_and_blur(self):
        header, image, truth = scene(7)
        h, w = image.shape
        src = np.float32([[0, 0], [w-1, 0], [w-1, h-1], [0, h-1]])
        dst = np.float32([[80, 30], [940, 90], [880, 810], [40, 750]])
        transform = cv2.getPerspectiveTransform(src, dst)
        warped = cv2.warpPerspective(image, transform, (1000, 880), borderValue=80)
        truth = cv2.perspectiveTransform(truth.reshape(1, -1, 2), transform)[0]
        for sigma in (0.0, 1.0, 2.4):
            with self.subTest(blur=sigma):
                sample = cv2.GaussianBlur(warped, (0, 0), sigma) if sigma else warped.copy()
                noise = np.random.default_rng(19).normal(0, 2, sample.shape)
                sample = np.clip(0.75 * sample + 25 + noise, 0, 255).astype(np.uint8)
                self.assert_corners(detect(header, sample), truth, 30, 0.8)

    def test_occluded_board_does_not_renumber_remaining_corners(self):
        header, image, truth = scene(7)
        image[:, :350] = 80
        result = detect(header, image)
        self.assert_corners(result, truth, 18)
        self.assertTrue(all(p["id"] % 7 >= 3 for p in result["reference"]["corners"]))

    def test_no_board_plain_sheet_wrong_dictionary_and_wrong_layout(self):
        header, image, _ = scene()
        plain = np.full_like(image, 80)
        plain[80:680, 90:890] = 255
        _, wrong_dictionary, _ = scene(dictionary=cv2.aruco.DICT_5X5_50)
        _, wrong_layout, _ = scene(legacy=True)
        for sample in (np.full_like(image, 80), plain, wrong_dictionary, wrong_layout):
            self.assertLess(len(detect(header, sample)["reference"]["corners"]), 6)

    def test_duplicate_boards_and_wrong_board_dimensions_refuse(self):
        header, image, _ = scene()
        with self.assertRaisesRegex(ValueError, "AMBIGUOUS_BOARD"):
            detect(header, np.concatenate((image, image), axis=1))
        header["sheet"]["board"]["squares_x"] = 4
        header["sheet"]["board"]["size_mm"] = [88, 132]
        with self.assertRaisesRegex(ValueError, "BOARD_MISMATCH"):
            detect(header, image)

    def test_bad_sidecar_and_pixels_refuse(self):
        for key, value in [("dictionary", "not-a-dictionary"), ("squares_x", True),
                           ("square_mm", float("nan")), ("marker_mm", 22),
                           ("legacy_pattern", "false"), ("size_mm", [1, 2]),
                           ("origin_mm", [0, float("inf")]), ("ids", [4, 3, 2, 1])]:
            header, image, _ = scene()
            header["sheet"]["board"][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                detect(header, image)
        header, image, _ = scene()
        with self.assertRaisesRegex(ValueError, "length"):
            adapter.detect(header, b"not pixels")

    def test_isolated_process_protocol(self):
        header, image, truth = scene()
        wire = json.dumps(header).encode() + b"\n" + cv2.cvtColor(image, cv2.COLOR_GRAY2BGR).tobytes()
        result = subprocess.run([sys.executable, "-I", "-c", SOURCE.read_text()],
                                input=wire, capture_output=True, timeout=30, check=True)
        self.assertEqual(result.stderr, b"")
        self.assert_corners(json.loads(result.stdout), truth, 35)
        bad = subprocess.run([sys.executable, "-I", "-c", SOURCE.read_text()],
                             input=b"{}\n", capture_output=True, timeout=30, check=True)
        self.assertIn("error", json.loads(bad.stdout))


if __name__ == "__main__":
    unittest.main(verbosity=2)
