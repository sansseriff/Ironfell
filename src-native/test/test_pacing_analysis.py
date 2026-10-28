"""Frame IDs must survive idle frames and asynchronous trace row ordering."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("pacing", Path(__file__).with_name("analyze-pacing.py"))
pacing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pacing)


def tick(frame, start, rendered=True):
    return dict(name="tick", frame=frame, start=start, end=start + .001,
                target=start + .016, callback=start - .005, rendered=rendered,
                input_received=start - .002)


def submit(frame, start):
    return dict(name="render_submit", frame=frame, start=start, end=start + .003)


class AnalysisTests(unittest.TestCase):
    def test_sparse_renders_join_by_host_frame(self):
        result = pacing.analyze(dict(schema=2, epoch=0, rows=[
            submit(1, 1.001), tick(1, 1), tick(2, 2, False),
            tick(3, 3), submit(3, 3.002)]), warmup=0)
        self.assertEqual(result["matchedSubmissions"], 2)
        self.assertEqual(result["hostToCpuPresentReturnMs"]["max"], 5)
        self.assertEqual(result["inputReceiptToCpuPresentReturnMs"]["max"], 7)
        self.assertEqual(result["scheduledWaitMs"]["p50"], 5)
        self.assertEqual(result["cpuDeadlineMisses"], 0)

    def test_submission_at_warmup_boundary_is_retained(self):
        result = pacing.analyze(dict(schema=2, epoch=0, rows=[
            tick(1, 4.99), submit(1, 5), tick(2, 5.01), submit(2, 5.02)]))
        self.assertEqual(result["matchedSubmissions"], 1)
        self.assertEqual(result["cpuDeadlineSamples"], 1)

    def test_late_submissions_and_missing_renders_are_visible(self):
        result = pacing.analyze(dict(schema=2, epoch=0, rows=[
            tick(1, 1), submit(1, 1.020), tick(2, 2)]), warmup=0)
        self.assertEqual(result["renderedTicks"], 2)
        self.assertEqual(result["matchedSubmissions"], 1)
        self.assertEqual(result["cpuDeadlineMisses"], 1)


if __name__ == "__main__":
    unittest.main()
