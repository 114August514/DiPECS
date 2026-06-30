import unittest
from pathlib import Path

import param_optimization


class ParamOptimizationTests(unittest.TestCase):
    def test_build_bench_command_emits_each_option_once(self):
        command = param_optimization.build_bench_command(
            bench_path=Path("/tmp/llama-bench"),
            model_path=Path("/tmp/model.gguf"),
            threads=4,
            batch_size=32,
            n_prompt=512,
            n_gen=64,
            repetitions=3,
            use_mmap=False,
        )

        expected_values = {
            "--threads": "4",
            "--batch-size": "32",
            "--n-prompt": "512",
            "--n-gen": "64",
            "--repetitions": "3",
            "--mmap": "0",
        }
        for option, value in expected_values.items():
            self.assertEqual(command.count(option), 1)
            self.assertEqual(command[command.index(option) + 1], value)

    def test_select_records_matches_requested_configuration(self):
        records = [
            {
                "n_threads": 12,
                "n_batch": 64,
                "n_prompt": 128,
                "n_gen": 0,
                "avg_ts": 200.0,
            },
            {
                "n_threads": 4,
                "n_batch": 64,
                "n_prompt": 128,
                "n_gen": 0,
                "avg_ts": 150.0,
            },
            {
                "n_threads": 12,
                "n_batch": 64,
                "n_prompt": 0,
                "n_gen": 64,
                "avg_ts": 35.0,
            },
            {
                "n_threads": 4,
                "n_batch": 64,
                "n_prompt": 0,
                "n_gen": 64,
                "avg_ts": 31.0,
            },
        ]

        prompt_record, generation_record = param_optimization.select_records(
            records,
            threads=4,
            batch_size=64,
            n_prompt=128,
            n_gen=64,
        )

        self.assertEqual(prompt_record["avg_ts"], 150.0)
        self.assertEqual(generation_record["avg_ts"], 31.0)


if __name__ == "__main__":
    unittest.main()
