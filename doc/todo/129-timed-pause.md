# TODO 129. Timed pause between workload steps

Let a native config schedule a quiet interval anywhere in its sequence. For example, a write-heavy correctness step runs to completion, all TMR test workers sleep for 10 seconds, then a read-heavy correctness step starts on the same allocation. Repeating the sequence exercises both the drop in load and its return.

The first version needs one duration in seconds. Wait once per occurrence after all workers finish the preceding step. A stop request or run deadline should interrupt a long wait promptly. Show the pause and elapsed time in the run record so it is clear when load resumed. Keep the seal in place and count the pause as zero tested bytes.

The existing [Refresh wait](../../dist/docs/tests/refresh-and-exchange.md) measures retention within a test. A sequence pause lets users choose the workloads on either side. Verify the new step with a repeated load–pause–load config and a run stopped during a pause.
