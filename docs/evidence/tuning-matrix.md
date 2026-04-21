# Windows Tuning Evidence Matrix

| Setting | Status | Included | Reason |
| --- | --- | --- | --- |
| DSCP/QoS policy helpers | supported | yes | operator-controlled and easy to verify |
| Local QoS marking registry enablement | supported | yes | required for local QoS policy behavior |
| NIC power-saving disables (`EEE`, `GreenEthernet`, `PowerSavingMode`) | conservative | yes in `Measured` | low-risk endpoint-side consistency check |
| High performance power plan | situational | yes in `Measured` | optional and reversible, useful for comparison runs |
| Interrupt moderation disable | excluded | no | successor analysis did not support broad default use |
| `NetworkThrottlingIndex` tweak | excluded | no | not evidence-backed for this suite |
| UDP receive offload disable | excluded | no | too situational for default diagnostics use |
| folklore registry bundles | excluded | no | not defensible without suite-local evidence |
