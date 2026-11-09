import importlib.util
import pathlib
import tempfile
import unittest
import ctypes


spec = importlib.util.spec_from_file_location(
    "profile_desktop", pathlib.Path(__file__).parents[1] / "profile-desktop.py"
)
profiler = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profiler)


class ProfileTests(unittest.TestCase):
    def test_apple_silicon_timebase_converts_only_times(self):
        values = {"user_ns": 24_000_000, "system_ns": 12_000_000,
                  "qos_background_ns": 6_000_000, "started": 42,
                  "interrupt_wakeups": 100, "instructions": 1000}
        converted = profiler.normalize_times(values, 125, 3)
        self.assertEqual(converted["user_ns"], 1_000_000_000)
        self.assertEqual(converted["system_ns"], 500_000_000)
        self.assertEqual(converted["qos_background_ns"], 250_000_000)
        self.assertEqual(converted["started"], 42)
        self.assertEqual(converted["interrupt_wakeups"], 100)
        self.assertEqual(converted["instructions"], 1000)
        self.assertEqual(profiler.normalize_times(values, 1, 1), values)

    def test_darwin_abi_sizes(self):
        self.assertEqual(ctypes.sizeof(profiler.Coalition), 40)
        self.assertEqual(ctypes.sizeof(profiler.Usage), 296)

    def test_only_owned_webkit_processes_are_selected(self):
        class API:
            def coalition(self, pid):
                return {2: 10, 3: 99, 4: 10}.get(pid)

        rows = {1: (0, "beep-desktop"),
                2: (0, "/system/com.apple.WebKit.WebContent"),
                3: (0, "/system/com.apple.WebKit.WebContent"),
                4: (0, "/system/com.apple.WebKit.GPU"),
                5: (1, "agent"), 6: (5, "tool")}
        self.assertEqual(profiler.discover(rows, 1, 10, API()),
                         {1: "native", 2: "renderer", 4: "gpu_service", 5: "workload", 6: "workload"})
        self.assertEqual(profiler.discover(rows, 1, None, API()),
                         {1: "native", 5: "workload", 6: "workload"})

    def test_reused_pid_starts_new_baseline(self):
        self.assertIsNone(profiler.delta({"started": 1}, {"started": 2}))
        self.assertEqual(profiler.delta({"started": 1, "user_ns": 10},
                                        {"started": 1, "user_ns": 5}), {"user_ns": 0})

    def test_log_reader_tracks_commands_without_emitting_arguments(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "app.log"
            path.write_text('command trace: {"command":"old","stage":"requested"}\n')
            reader = profiler.LogReader(path)
            with path.open("a") as handle:
                handle.write('command trace: {"command":"agent_view","stage":"requested","args":"private"}\n')
                handle.write('command trace: {"command":"agent_view","stage":"returned","ms":5}\n')
            reader.read()
            self.assertEqual(reader.calls, {"agent_view": 1})
            self.assertEqual(reader.wait_ms, {"agent_view": 5})
            path.rename(path.with_suffix(".old"))
            path.write_text('command trace: {"command":"center_home","stage":"requested"}\n')
            reader.read()
            self.assertEqual(reader.calls, {"agent_view": 1, "center_home": 1})
            reader.close()

    def test_metrics_use_only_complete_snapshots_and_report_their_own_window(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "metrics.log"
            def snapshot(stamp, calls, seconds):
                return (f'# snapshot unix_seconds={stamp}\n'
                        f'beep_desktop_duration_seconds_count{{kind="command",name="agent_view",outcome="ok"}} {calls}\n'
                        f'beep_desktop_duration_seconds_sum{{kind="command",name="agent_view",outcome="ok"}} {seconds}\n'
                        '# end snapshot\n')
            path.write_text(snapshot(100, 2, 0.1))
            before = profiler.metric_snapshot(path)
            path.write_text(snapshot(100, 2, 0.1) + snapshot(160, 5, 0.4)
                            + '# snapshot unix_seconds=220\n')
            after = profiler.metric_snapshot(path)
            self.assertEqual(after["unix_seconds"], 160)
            change = profiler.metric_changes(before, after)
            self.assertEqual(change["seconds"], 60)
            self.assertEqual(change["commands"]["agent_view"]["calls"], 3)
            self.assertAlmostEqual(change["commands"]["agent_view"]["wait_seconds"], 0.3)
            self.assertFalse(profiler.metric_changes(after, after)["available"])


if __name__ == "__main__":
    unittest.main()
