
  # Benchmark results - Q3

  Latency percentiles across the three deployment targets. Lower is
 better. All numbers are milliseconds from cold start unless noted.

  | Target | p50 | p90 | p99 |
| --- | --- | --- | --- |
| linux-amd64 | 41 | 55 | 72 |
| linux-arm64 | 46 | 60 | 79 |
| darwin-arm64 | 38 | 49 | 63 |

  ## Notes

  - Runs performed on dedicated hardware, three iterations each,
   median reported.
- The pipe target column uses a legacy attribute name retained for
   compatibility with old exporters.

  | Target | Mean | Std |
| --- | --- | --- |
| linux-amd64 | 18,204 | 611 |
| linux-arm64 | 17,883 | 590 |
