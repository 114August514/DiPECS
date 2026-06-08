import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import concurrency_stress_test
import ray_batch_inference


class RayExperimentTests(unittest.TestCase):
    def test_nearest_rank_p95_does_not_use_max_for_twenty_samples(self):
        values = list(range(1, 21))

        self.assertEqual(
            ray_batch_inference.nearest_rank_percentile(values, 0.95),
            19,
        )
        self.assertEqual(
            concurrency_stress_test.nearest_rank_percentile(values, 0.95),
            19,
        )

    def test_prompt_loaders_respect_explicit_limit(self):
        with tempfile.TemporaryDirectory() as directory:
            prompt_path = Path(directory) / "prompts.jsonl"
            with prompt_path.open("w", encoding="utf-8") as output:
                for index in range(30):
                    output.write(
                        json.dumps(
                            {
                                "id": f"prompt-{index}",
                                "prompt": "test",
                                "max_tokens": 8,
                            }
                        )
                        + "\n"
                    )

            self.assertEqual(
                len(ray_batch_inference.load_prompts(prompt_path, limit=20)),
                20,
            )
            self.assertEqual(
                len(concurrency_stress_test.load_prompts(prompt_path, limit=20)),
                20,
            )

    def test_remote_collection_starts_timer_before_submission(self):
        events = []

        def submit():
            events.append("submit")
            return ["object-ref"]

        def perf_counter():
            events.append("clock")
            return 10.0 if events.count("clock") == 1 else 12.5

        with (
            mock.patch.object(
                ray_batch_inference.time,
                "perf_counter",
                side_effect=perf_counter,
            ),
            mock.patch.object(
                ray_batch_inference.ray,
                "get",
                return_value=[{"success": True}],
            ),
        ):
            results, elapsed_ms = ray_batch_inference.collect_remote_results(submit)

        self.assertEqual(events, ["clock", "submit", "clock"])
        self.assertEqual(results, [{"success": True}])
        self.assertEqual(elapsed_ms, 2500.0)

    def test_cli_supports_thirty_prompt_load_balancing_run(self):
        arguments = ray_batch_inference.parse_arguments(
            [
                "--prompt-count",
                "30",
                "--strategies",
                "round_robin",
                "latency_based",
                "--output",
                "data/results/ray-loadbalance-30/detail.jsonl",
            ]
        )

        self.assertEqual(arguments.prompt_count, 30)
        self.assertEqual(
            arguments.strategies,
            ["round_robin", "latency_based"],
        )
        self.assertEqual(
            arguments.output,
            Path("data/results/ray-loadbalance-30/detail.jsonl"),
        )

    def test_latency_mapping_allocates_more_requests_to_faster_server(self):
        mapping = ray_batch_inference.latency_based_mapping(
            prompt_count=30,
            s1_latency_ms=3000,
            s2_latency_ms=4000,
        )

        self.assertEqual(len(mapping), 30)
        self.assertEqual(mapping.count("s1"), 17)
        self.assertEqual(mapping.count("s2"), 13)

    def test_local_cluster_connection_does_not_package_working_directory(self):
        self.assertEqual(
            ray_batch_inference.ray_init_options(),
            {"address": "auto"},
        )


if __name__ == "__main__":
    unittest.main()
