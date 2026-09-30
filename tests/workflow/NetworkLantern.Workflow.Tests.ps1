Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

BeforeAll {
  $script:RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
  $script:ModulePath = Join-Path $script:RepoRoot 'src/powershell/workflow/NetworkLantern.Workflow/NetworkLantern.Workflow.psd1'
  $script:WorkflowModule = Import-Module -Name $script:ModulePath -Force -PassThru
  $script:InvokeChildBootstrapEnvelope = {
    param([string]$EncodedBootstrap, [string]$Envelope)

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = 'pwsh'
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardInput = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in @('-NoProfile', '-NonInteractive', '-EncodedCommand', $EncodedBootstrap)) {
      $null = $startInfo.ArgumentList.Add($argument)
    }

    $process = [System.Diagnostics.Process]::Start($startInfo)
    try {
      $process.StandardInput.Write($Envelope)
      $process.StandardInput.Close()
      $stdout = $process.StandardOutput.ReadToEnd()
      $stderr = $process.StandardError.ReadToEnd()
      $process.WaitForExit()
      return [pscustomobject]@{ ExitCode = $process.ExitCode; Output = $stdout + $stderr }
    } finally {
      $process.Dispose()
    }
  }
  $script:NewSizedChildEnvelope = {
    param([int]$ByteLength)

    $baseEnvelope = [ordered]@{ Capability = 'Unsupported'; Parameters = [ordered]@{ Padding = '' } }
    $baseJson = ConvertTo-Json -InputObject $baseEnvelope -Compress
    $paddingLength = $ByteLength - [System.Text.Encoding]::UTF8.GetByteCount($baseJson)
    $paddingLength | Should -BeGreaterOrEqual 0
    $envelope = [ordered]@{ Capability = 'Unsupported'; Parameters = [ordered]@{ Padding = ('x' * $paddingLength) } }
    $json = ConvertTo-Json -InputObject $envelope -Compress
    [System.Text.Encoding]::UTF8.GetByteCount($json) | Should -Be $ByteLength
    return $json
  }
  $script:WriteUtf8File = {
    param([string]$Path, [string]$Content)

    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
  }
  $script:NewSizedWorkflowProfile = {
    param([int]$ByteLength)

    $prefix = '{"path":{"hostsIPv4":["exact.example"],"padding":"'
    $suffix = '"}}'
    $paddingLength = $ByteLength - [System.Text.Encoding]::UTF8.GetByteCount($prefix + $suffix)
    $paddingLength | Should -BeGreaterOrEqual 0
    $json = $prefix + ('x' * $paddingLength) + $suffix
    [System.Text.Encoding]::UTF8.GetByteCount($json) | Should -Be $ByteLength
    return $json
  }
}

Describe 'Network Lantern workflow module' {
  It 'uses explicit values ahead of profile values and warns for ignored keys' {
    $profilePath = Join-Path $TestDrive 'workflow.json'
    Set-Content -LiteralPath $profilePath -Encoding utf8 -Value @'
{
  "path": { "hostsIPv4": ["profile.example"], "unknown": true },
  "unknownSection": { "value": 1 }
}
'@

    $result = & $script:WorkflowModule {
      param($path)
      $workflowProfile = Import-NetworkLanternWorkflowProfile -ProfilePath $path
      $warnings = @(Write-NetworkLanternWorkflowProfileWarnings -ProfileMap $workflowProfile 3>&1)
      $section = Get-NetworkLanternProfileSection -ProfileMap $workflowProfile -Name 'path'
      $effective = @(Resolve-NetworkLanternEffectiveValue -ExplicitValue @('explicit.example') -ExplicitValueWasProvided $true -Section $section -Key 'hostsIPv4' -Fallback @())
      [pscustomobject]@{ Warnings = $warnings; Effective = $effective }
    } $profilePath

    $result.Effective | Should -Be @('explicit.example')
    ($result.Warnings | Out-String) | Should -Match 'path.unknown'
    ($result.Warnings | Out-String) | Should -Match 'unknownSection'
  }

  It 'accepts a workflow profile at the exact 1 MiB byte limit' {
    $profilePath = Join-Path $TestDrive 'exact-limit.json'
    & $script:WriteUtf8File -Path $profilePath -Content (& $script:NewSizedWorkflowProfile -ByteLength 1MB)

    $workflowProfile = & $script:WorkflowModule {
      param($path)
      Import-NetworkLanternWorkflowProfile -ProfilePath $path
    } $profilePath

    @($workflowProfile.path.hostsIPv4) | Should -Be @('exact.example')
  }

  It 'rejects a workflow profile at 1 MiB plus one byte before parsing' {
    $profilePath = Join-Path $TestDrive 'over-limit.json'
    & $script:WriteUtf8File -Path $profilePath -Content (& $script:NewSizedWorkflowProfile -ByteLength ((1MB) + 1))

    {
      & $script:WorkflowModule {
        param($path)
        Import-NetworkLanternWorkflowProfile -ProfilePath $path
      } $profilePath
    } | Should -Throw '*ProfilePath exceeds maximum size*'
  }

  It 'reads an opened workflow profile even after its path is replaced' {
    $profilePath = Join-Path $TestDrive 'replaced-profile.json'
    $replacementPath = Join-Path $TestDrive 'replacement-profile.json'
    $originalProfile = '{"path":{"hostsIPv4":["original.example"]}}'
    $replacementProfile = '{"path":{"hostsIPv4":["replacement.example"]}}'
    & $script:WriteUtf8File -Path $profilePath -Content $originalProfile
    & $script:WriteUtf8File -Path $replacementPath -Content $replacementProfile

    $stream = [System.IO.File]::Open(
      $profilePath,
      [System.IO.FileMode]::Open,
      [System.IO.FileAccess]::Read,
      ([System.IO.FileShare]::ReadWrite -bor [System.IO.FileShare]::Delete)
    )
    try {
      Move-Item -LiteralPath $replacementPath -Destination $profilePath -Force
      $profileText = & $script:WorkflowModule {
        param($inputStream)
        Read-NetworkLanternWorkflowProfileText -Stream $inputStream
      } $stream
    } finally {
      $stream.Dispose()
    }

    $profileText | Should -Be $originalProfile
    [System.IO.File]::ReadAllText($profilePath) | Should -Be $replacementProfile
  }

  It 'rejects a profile that grows past the limit after its handle is opened' {
    $profilePath = Join-Path $TestDrive 'growing-profile.json'
    & $script:WriteUtf8File -Path $profilePath -Content (& $script:NewSizedWorkflowProfile -ByteLength 1MB)

    $stream = [System.IO.File]::Open(
      $profilePath,
      [System.IO.FileMode]::Open,
      [System.IO.FileAccess]::Read,
      ([System.IO.FileShare]::ReadWrite -bor [System.IO.FileShare]::Delete)
    )
    try {
      [System.IO.File]::AppendAllText($profilePath, 'x', [System.Text.UTF8Encoding]::new($false))
      {
        & $script:WorkflowModule {
          param($inputStream)
          Read-NetworkLanternWorkflowProfileText -Stream $inputStream
        } $stream
      } | Should -Throw '*ProfilePath exceeds maximum size*'
    } finally {
      $stream.Dispose()
    }
  }

  It 'rejects malformed and overly nested workflow profile JSON' {
    $malformedPath = Join-Path $TestDrive 'malformed-profile.json'
    $nestedPath = Join-Path $TestDrive 'nested-profile.json'
    & $script:WriteUtf8File -Path $malformedPath -Content '{"path":'

    $nestedJson = '"value"'
    for ($index = 0; $index -lt 17; $index++) {
      $nestedJson = '{"level":' + $nestedJson + '}'
    }
    & $script:WriteUtf8File -Path $nestedPath -Content $nestedJson

    {
      & $script:WorkflowModule {
        param($path)
        Import-NetworkLanternWorkflowProfile -ProfilePath $path
      } $malformedPath
    } | Should -Throw
    {
      & $script:WorkflowModule {
        param($path)
        Import-NetworkLanternWorkflowProfile -ProfilePath $path
      } $nestedPath
    } | Should -Throw
  }

  It 'builds an ordered, non-executing baseline plan' {
    $plan = New-NetworkLanternWorkflowPlan -Workflow Baseline -IperfTarget iperf3.example.net -DryRun `
      -OutRoot (Join-Path $TestDrive 'plan-artifacts') `
      -ExplicitParameters @{ Workflow = 'Baseline'; IperfTarget = 'iperf3.example.net'; DryRun = $true }

    $plan.Workflow | Should -Be 'Baseline'
    @($plan.Steps.Capability) | Should -Be @('Path', 'Throughput')
    $plan.Steps[0].Parameters.LogDirectory | Should -Be (Join-Path $TestDrive 'plan-artifacts/path')
    $plan.Steps[0].Parameters.DryRun | Should -BeTrue
    $plan.Steps[1].Parameters.WhatIf | Should -BeTrue
    (Test-Path -LiteralPath (Join-Path $TestDrive 'plan-artifacts')) | Should -BeFalse
  }

  It 'validates profile values after explicit-over-profile precedence is resolved' {
    $profilePath = Join-Path $TestDrive 'overridden-invalid-profile.json'
    & $script:WriteUtf8File -Path $profilePath -Content '{"throughput":{"target":42,"port":70000,"protocol":"SCTP","maxTotalTests":1000001}}'

    $plan = New-NetworkLanternWorkflowPlan -Workflow Throughput -ProfilePath $profilePath `
      -IperfTarget 'explicit.example' -IperfPort 5202 -ThroughputProtocol TCP -ThroughputMaxTotalTests 25 `
      -OutRoot $TestDrive -ExplicitParameters @{
        Workflow = 'Throughput'; IperfTarget = 'explicit.example'; IperfPort = 5202
        ThroughputProtocol = 'TCP'; ThroughputMaxTotalTests = 25
      }

    $plan.Steps[0].Parameters.Target | Should -Be 'explicit.example'
    $plan.Steps[0].Parameters.Port | Should -Be 5202
    $plan.Steps[0].Parameters.Protocol | Should -Be 'TCP'
    $plan.Steps[0].Parameters.MaxTotalTests | Should -Be 25
  }

  It 'rejects invalid resolved profile values before constructing a child plan' -TestCases @(
    @{ Json = '{"path":{"protocols":["IPv5"]}}'; Expected = 'path.protocols' }
    @{ Json = '{"throughput":{"target":42}}'; Expected = 'throughput.target' }
    @{ Json = '{"throughput":{"port":70000}}'; Expected = 'throughput.port' }
    @{ Json = '{"throughput":{"protocol":"SCTP"}}'; Expected = 'throughput.protocol' }
    @{ Json = '{"throughput":{"maxTotalTests":1000001}}'; Expected = 'throughput.maxTotalTests' }
    @{ Json = '{"windowsTuning":{"action":"Reset"}}'; Expected = 'windowsTuning.action' }
    @{ Json = '{"windowsTuning":{"profile":"Fast"}}'; Expected = 'windowsTuning.profile' }
    @{ Json = '{"windowsTuning":{"udpPorts":[-1]}}'; Expected = 'windowsTuning.udpPorts' }
  ) {
    param($Json, $Expected)

    $profilePath = Join-Path $TestDrive ("invalid-{0}.json" -f [guid]::NewGuid().ToString('N'))
    & $script:WriteUtf8File -Path $profilePath -Content $Json

    {
      New-NetworkLanternWorkflowPlan -Workflow Path -ProfilePath $profilePath -OutRoot $TestDrive `
        -ExplicitParameters @{ Workflow = 'Path'; ProfilePath = $profilePath }
    } | Should -Throw "*$Expected*"
  }

  It 'maps the trusted workflow throughput budget from a profile to MaxTotalTests' {
    $profilePath = Join-Path $TestDrive 'throughput-budget.json'
    & $script:WriteUtf8File -Path $profilePath -Content '{"throughput":{"target":"iperf3.example.net","maxTotalTests":321}}'

    $plan = New-NetworkLanternWorkflowPlan -Workflow Throughput -ProfilePath $profilePath -OutRoot $TestDrive `
      -ExplicitParameters @{ Workflow = 'Throughput'; ProfilePath = $profilePath }
    . (Join-Path $script:RepoRoot 'apps/workflow/Private/WorkflowApplication.ps1')
    $descriptor = Test-NetworkLanternWorkflowCapabilityStep -Step $plan.Steps[0]

    $plan.Steps[0].Parameters.MaxTotalTests.GetType() | Should -Be ([int])
    $plan.Steps[0].Parameters.MaxTotalTests | Should -Be 321
    $descriptor.AllowedParameters | Should -Contain 'MaxTotalTests'
  }

  It 'forwards the root throughput budget through the trusted child contract' {
    $outRoot = Join-Path $TestDrive 'budget-artifacts'

    $output = & pwsh -NoProfile -NonInteractive -File (Join-Path $script:RepoRoot 'Invoke-NetworkLantern.ps1') `
      -Workflow Throughput -IperfTarget 'iperf3.example.net' -ThroughputMaxTotalTests 1 -DryRun -OutRoot $outRoot 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'MaxTotalTests: 1\. Within budget: False'
    Test-Path -LiteralPath $outRoot | Should -BeFalse
  }

  It 'applies profile precedence through the stable adapter' {
    $profilePath = Join-Path $TestDrive 'adapter-workflow.json'
    Set-Content -LiteralPath $profilePath -Encoding utf8 -Value @'
{
  "path": {
    "hostsIPv4": ["profile.example"],
    "protocols": ["IPv4"],
    "rounds": ["Standard"],
    "unknown": true
  }
}
'@

    $output = & pwsh -NoProfile -NonInteractive -File (Join-Path $script:RepoRoot 'Invoke-NetworkLantern.ps1') `
      -Workflow Path -ProfilePath $profilePath -HostsIPv4 explicit.example -DryRun -OutRoot (Join-Path $TestDrive 'adapter-artifacts') 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'Unknown workflow profile key.*path.unknown'
    ($output | Out-String) | Should -Match 'IPv4 hosts: explicit.example'
  }

  It 'keeps workflow dry-run previews side-effect free' {
    $outRoot = Join-Path $TestDrive 'artifacts'
    $output = & pwsh -NoProfile -NonInteractive -File (Join-Path $script:RepoRoot 'Invoke-NetworkLantern.ps1') `
      -Workflow Path -HostsIPv4 example.com -Protocols IPv4 -Rounds Standard -DryRun -Quiet -OutRoot $outRoot 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'Dry-run only'
    (Test-Path -LiteralPath $outRoot) | Should -BeFalse
  }

  It 'accepts every planner step through the canonical capability descriptors' {
    . (Join-Path $script:RepoRoot 'apps/workflow/Private/WorkflowApplication.ps1')
    $descriptors = Get-NetworkLanternWorkflowCapabilityDescriptors
    @($descriptors.Keys) | Should -Be @('Path', 'Throughput', 'WindowsTuning')
    $trustedAdapters = Get-NetworkLanternTrustedCapabilityAdapters -RepositoryRoot $script:RepoRoot

    foreach ($capability in @($descriptors.Keys)) {
      $trustedAdapters[$capability] | Should -Be (Join-Path $script:RepoRoot $descriptors[$capability].AdapterRelativePath)
    }

    foreach ($workflow in @('Triage', 'Path', 'Throughput', 'Baseline', 'WindowsTuning')) {
      $plan = New-NetworkLanternWorkflowPlan -Workflow $workflow -IperfTarget 'iperf3.example.net' -OutRoot $TestDrive `
        -ExplicitParameters @{ Workflow = $workflow; IperfTarget = 'iperf3.example.net' }
      foreach ($step in @($plan.Steps)) {
        $descriptor = Test-NetworkLanternWorkflowCapabilityStep -Step $step
        $descriptors.Contains($step.Capability) | Should -BeTrue
        @($step.Parameters.Keys | Where-Object { $_ -notin @($descriptor.AllowedParameters) }) | Should -BeNullOrEmpty
      }
    }
  }

  It 'rejects a planner step with a parameter outside its canonical descriptor before process creation' {
    . (Join-Path $script:RepoRoot 'apps/workflow/Private/WorkflowApplication.ps1')
    $step = [pscustomobject]@{ Capability = 'Path'; Parameters = @{ ScriptPath = 'not-allowed.ps1' } }

    { Invoke-NetworkLanternCapabilityChild -Step $step -RepositoryRoot $script:RepoRoot } |
      Should -Throw '*Unsupported parameter*'
  }

  It 'reads an exactly maximum-sized child envelope before capability validation' {
    . (Join-Path $script:RepoRoot 'apps/workflow/Private/WorkflowApplication.ps1')
    $encodedBootstrap = New-NetworkLanternCapabilityChildBootstrap -RepositoryRoot $script:RepoRoot
    $envelope = & $script:NewSizedChildEnvelope -ByteLength 1MB
    $result = & $script:InvokeChildBootstrapEnvelope -EncodedBootstrap $encodedBootstrap -Envelope $envelope

    $result.ExitCode | Should -Not -Be 0
    $result.Output | Should -Match "Unsupported workflow capability 'Unsupported'"
    $result.Output | Should -Not -Match 'envelope exceeds maximum size'
  }

  It 'rejects a child envelope at maximum plus one byte without consuming an unbounded stream' {
    . (Join-Path $script:RepoRoot 'apps/workflow/Private/WorkflowApplication.ps1')
    $encodedBootstrap = New-NetworkLanternCapabilityChildBootstrap -RepositoryRoot $script:RepoRoot
    $envelope = & $script:NewSizedChildEnvelope -ByteLength ((1MB) + 1)
    $result = & $script:InvokeChildBootstrapEnvelope -EncodedBootstrap $encodedBootstrap -Envelope $envelope

    $result.ExitCode | Should -Not -Be 0
    $result.Output | Should -Match 'Workflow child envelope exceeds maximum size'
  }

  It 'makes child parameter binding errors terminating failures' {
    . (Join-Path $script:RepoRoot 'apps/workflow/Private/WorkflowApplication.ps1')
    $encodedBootstrap = New-NetworkLanternCapabilityChildBootstrap -RepositoryRoot $script:RepoRoot
    $envelope = ConvertTo-Json -Compress -InputObject @{
      Capability = 'WindowsTuning'
      Parameters = @{ Action = 'Unsupported' }
    }

    $result = & $script:InvokeChildBootstrapEnvelope -EncodedBootstrap $encodedBootstrap -Envelope $envelope

    $result.ExitCode | Should -Not -Be 0
    $result.Output | Should -Match 'Action|ValidateSet|validation set'
  }

  It 'makes child adapter invocation errors terminating failures' {
    . (Join-Path $script:RepoRoot 'apps/workflow/Private/WorkflowApplication.ps1')
    $fakeRoot = Join-Path $TestDrive 'throwing-child-root'
    $adapter = Join-Path $fakeRoot 'apps/path/Test-NetworkPath.ps1'
    New-Item -ItemType Directory -Path (Split-Path -Parent $adapter) -Force | Out-Null
    & $script:WriteUtf8File -Path $adapter -Content "throw 'injected adapter failure'"
    $encodedBootstrap = New-NetworkLanternCapabilityChildBootstrap -RepositoryRoot $fakeRoot
    $envelope = ConvertTo-Json -Compress -InputObject @{ Capability = 'Path'; Parameters = @{} }

    $result = & $script:InvokeChildBootstrapEnvelope -EncodedBootstrap $encodedBootstrap -Envelope $envelope

    $result.ExitCode | Should -Not -Be 0
    $result.Output | Should -Match 'injected adapter failure'
  }

  It 'initializes child native exit handling for a successful PowerShell adapter' {
    . (Join-Path $script:RepoRoot 'apps/workflow/Private/WorkflowApplication.ps1')
    $fakeRoot = Join-Path $TestDrive 'successful-child-root'
    $adapter = Join-Path $fakeRoot 'apps/path/Test-NetworkPath.ps1'
    New-Item -ItemType Directory -Path (Split-Path -Parent $adapter) -Force | Out-Null
    & $script:WriteUtf8File -Path $adapter -Content "Write-Output 'adapter completed'"
    $encodedBootstrap = New-NetworkLanternCapabilityChildBootstrap -RepositoryRoot $fakeRoot
    $envelope = ConvertTo-Json -Compress -InputObject @{ Capability = 'Path'; Parameters = @{} }

    $result = & $script:InvokeChildBootstrapEnvelope -EncodedBootstrap $encodedBootstrap -Envelope $envelope

    $result.ExitCode | Should -Be 0
    $result.Output | Should -Match 'adapter completed'
  }

  It 'rejects an absent child-process exit status instead of reporting root success' {
    . (Join-Path $script:RepoRoot 'apps/workflow/Private/WorkflowApplication.ps1')
    Mock pwsh {}
    $step = [pscustomobject]@{ Capability = 'Path'; Parameters = @{} }

    {
      Invoke-NetworkLanternCapabilityChild -Step $step -RepositoryRoot $script:RepoRoot
    } | Should -Throw '*did not return a process exit status*'
  }

  It 'rejects an oversized child envelope before starting a process' {
    . (Join-Path $script:RepoRoot 'apps/workflow/Private/WorkflowApplication.ps1')
    $step = [pscustomobject]@{
      Capability = 'Path'
      Parameters = @{ HostsIPv4 = @('x' * 1MB) }
    }

    { Invoke-NetworkLanternCapabilityChild -Step $step -RepositoryRoot $script:RepoRoot } |
      Should -Throw '*envelope exceeds maximum size*'
  }

  It 'forwards the isolated child failure status through the stable adapter' {
    $output = & pwsh -NoProfile -NonInteractive -File (Join-Path $script:RepoRoot 'Invoke-NetworkLantern.ps1') `
      -Workflow WindowsTuning -TuningAction Verify 2>&1

    $LASTEXITCODE | Should -Be 1
    ($output | Out-String) | Should -Not -BeNullOrEmpty
  }
}
