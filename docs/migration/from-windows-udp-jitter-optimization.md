# Migration from windows-udp-jitter-optimization

- the repo is no longer a jitter-tweak product
- the public tuning surface is now generic and conservative
- game-specific presets and aggressive tweak bundles are intentionally not carried forward
- backup, restore, QoS helpers, and safe endpoint policy checks survive as the optional `windows-tuning` module
