### Changed

- Desktop: the Beep tools for coding agents now have one tool per action, such as `list_machines`, `remote_run`, `remote_run_status`, `send_files`, `start_agent`, `device_tap`, `beep_logs`, `generate_image`, `generate_3d`, `rig_3d` and `list_models`. Each tool takes only its own arguments, so agents mix up arguments less often.
- Desktop: the `plugins` tool is removed, so coding agents can no longer list, create or call Beep plugins.
