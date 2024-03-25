Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# workflows entrypoint

# current lane: workflows
function Invoke-Workflows {
    [CmdletBinding()]
    param()
}

# current lane: powershell
function Invoke-Powershell {
    [CmdletBinding()]
    param()
}

# current lane: path
function Invoke-Path {
    [CmdletBinding()]
    param()
}

# current lane: throughput
function Invoke-Throughput {
    [CmdletBinding()]
    param()
}

# current lane: tuning
function Invoke-Tuning {
    [CmdletBinding()]
    param()
}

# current lane: split_path_throughput_and_tuning_into_dedicated_apps
function Invoke-SplitPathThroughputAndTuningIntoDedicatedApps {
    [CmdletBinding()]
    param()
}

# current lane: reporting
function Invoke-Reporting {
    [CmdletBinding()]
    param()
}

# current lane: pester
function Invoke-Pester {
    [CmdletBinding()]
    param()
}

# current lane: keep_backward_compatible_interfaces_around_path_throughput_and_tuning_into_dedicated_apps
function Invoke-KeepBackwardCompatibleInterfacesAroundPathThroughputAndTuningIntoDedicatedApps {
    [CmdletBinding()]
    param()
}
