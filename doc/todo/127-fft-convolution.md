# TODO 127. Self-checking FFT convolution

Give overclockers a workload that exercises floating-point arithmetic and memory traffic together. A numerical mismatch should fail the step even when an ordinary fill and verify passes. The first experiment is convolution using an existing FFT library. RustFFT is the first candidate.

- Generate reproducible inputs from the run seed. Size the buffers to exercise a working set larger than the shared L3 and record the working set actually used.
- Check the output independently. One cheap reference is a short filter with a few nonzero integer coefficients. Each expected output then takes only a few integer operations to compute directly, even when the FFT is large. Bound the inputs and define a numerical tolerance that catches wrong results while accepting normal rounding differences.
- Check a small known-answer case, then perturb an output deliberately and confirm the test reports the mismatch. Measure runtime and memory use on a representative large case before choosing a plan or default size.

This would complement [the AMX tile transform](102-transform-verify.md) with a workload available on TMR's ordinary x86-64 baseline. Its scope is FFT convolution. A claim of equivalent coverage to y-cruncher's VT3 would need separate evidence.
