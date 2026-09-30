BeforeAll {
  $script:RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
}

Describe 'Repository architecture boundaries' {
  It 'keeps the documented operator entrypoints stable' {
    $entrypoints = @(
      'Invoke-NetworkLantern.ps1'
      'apps/path/Test-NetworkPath.ps1'
      'apps/path/test-network-path.sh'
      'apps/throughput/Measure-NetworkThroughput.ps1'
      'apps/throughput/Measure-NetworkThroughput-GUI.ps1'
      'apps/windows-tuning/Invoke-NetworkPathTuning.ps1'
      'scripts/run-workflow.sh'
    )

    foreach ($relativePath in $entrypoints) {
      Join-Path $script:RepoRoot $relativePath | Should -Exist
    }
  }

  It 'defines each PowerShell capability through a module manifest' {
    $manifests = @(
      'src/powershell/path/NetworkLantern.Path/NetworkLantern.Path.psd1'
      'src/powershell/throughput/NetworkLantern.Throughput.psd1'
      'src/powershell/windows-tuning/NetworkLantern.WindowsTuning/NetworkLantern.WindowsTuning.psd1'
      'src/powershell/workflow/NetworkLantern.Workflow/NetworkLantern.Workflow.psd1'
    )

    foreach ($relativePath in $manifests) {
      $manifestPath = Join-Path $script:RepoRoot $relativePath
      $manifestPath | Should -Exist
      Test-ModuleManifest -Path $manifestPath -ErrorAction Stop | Should -Not -BeNullOrEmpty
    }
  }

  It 'keeps all PowerShell module loader inventories, manifests, and public exports aligned' {
    $modules = @(
      @{ Root = 'src/powershell/path/NetworkLantern.Path'; Manifest = 'NetworkLantern.Path.psd1'; PrivateInventory = 'privateLoadOrder'; PublicInventory = 'publicLoadOrder'; DirectPrivate = @() }
      @{ Root = 'src/powershell/throughput'; Manifest = 'NetworkLantern.Throughput.psd1'; PrivateInventory = 'privateScripts'; PublicInventory = 'publicScripts'; DirectPrivate = @() }
      @{ Root = 'src/powershell/windows-tuning/NetworkLantern.WindowsTuning'; Manifest = 'NetworkLantern.WindowsTuning.psd1'; PrivateInventory = 'privateLoadOrder'; PublicInventory = 'publicLoadOrder'; DirectPrivate = @('Constants.ps1') }
      @{ Root = 'src/powershell/workflow/NetworkLantern.Workflow'; Manifest = 'NetworkLantern.Workflow.psd1'; PrivateInventory = 'privateLoadOrder'; PublicInventory = 'publicLoadOrder'; DirectPrivate = @() }
    )

    foreach ($definition in $modules) {
      $moduleRoot = Join-Path $script:RepoRoot $definition.Root
      $manifestPath = Join-Path $moduleRoot $definition.Manifest
      $manifest = Test-ModuleManifest -Path $manifestPath -ErrorAction Stop
      $module = Import-Module -Name $manifestPath -Force -PassThru
      $manifestExports = @($manifest.ExportedFunctions.Keys | Sort-Object)
      $moduleExports = @($module.ExportedCommands.Keys | Sort-Object)
      $manifestExports.Count | Should -BeGreaterThan 0 -Because "$($manifest.Name) must declare its public contract"
      $moduleExports | Should -Be $manifestExports -Because "$($manifest.Name) manifest and runtime exports must agree"

      $privateInventory = @(& $module { param($name) foreach ($item in @(Get-Variable -Name $name -ValueOnly)) { $item } } $definition.PrivateInventory)
      $publicInventory = @(& $module { param($name) foreach ($item in @(Get-Variable -Name $name -ValueOnly)) { $item } } $definition.PublicInventory)
      $actualPrivate = @((Get-ChildItem -LiteralPath (Join-Path $moduleRoot 'Private') -File -Filter '*.ps1').Name | Sort-Object)
      $actualPublic = @((Get-ChildItem -LiteralPath (Join-Path $moduleRoot 'Public') -File -Filter '*.ps1').Name | Sort-Object)
      @(($privateInventory + $definition.DirectPrivate) | Sort-Object) | Should -Be $actualPrivate -Because "$($manifest.Name) private loader inventory must be explicit and complete"
      @($publicInventory | Sort-Object) | Should -Be $actualPublic -Because "$($manifest.Name) public loader inventory must be explicit and complete"

      $publicRoot = [System.IO.Path]::GetFullPath((Join-Path $moduleRoot 'Public')).TrimEnd([System.IO.Path]::DirectorySeparatorChar)
      foreach ($commandName in $moduleExports) {
        $command = Get-Command -Name $commandName -Module $module.Name -CommandType Function -ErrorAction Stop
        [string]::IsNullOrWhiteSpace([string]$command.ScriptBlock.File) | Should -BeFalse -Because "$commandName must be defined in a source file"
        $definitionPath = [System.IO.Path]::GetFullPath([string]$command.ScriptBlock.File)
        $definitionPath.StartsWith($publicRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase) |
          Should -BeTrue -Because "$commandName must be physically defined under Public"
      }
    }
  }

  It 'keeps the workflow adapter and module parameter surfaces aligned' {
    $adapterParameters = (Get-Command (Join-Path $script:RepoRoot 'Invoke-NetworkLantern.ps1')).Parameters.Keys | Sort-Object
    $workflowManifest = Join-Path $script:RepoRoot 'src/powershell/workflow/NetworkLantern.Workflow/NetworkLantern.Workflow.psd1'
    Import-Module -Name $workflowManifest -Force
    $moduleParameters = (Get-Command 'New-NetworkLanternWorkflowPlan').Parameters.Keys |
      Where-Object { $_ -ne 'ExplicitParameters' } |
      Sort-Object

    Compare-Object -ReferenceObject $adapterParameters -DifferenceObject $moduleParameters | Should -BeNullOrEmpty
    $adapterParameters | Should -Not -Contain 'ExplicitParameters'
  }

  It 'does not let operator adapters import module-private files or development helpers' {
    $adapterRoots = @(
      (Join-Path $script:RepoRoot 'apps')
      (Join-Path $script:RepoRoot 'Invoke-NetworkLantern.ps1')
    )
    $adapterFiles = foreach ($root in $adapterRoots) {
      if (Test-Path -LiteralPath $root -PathType Leaf) {
        Get-Item -LiteralPath $root
      }
      else {
        Get-ChildItem -LiteralPath $root -Recurse -File -Include '*.ps1'
      }
    }

    $prohibitedAdapterReferences = [ordered]@{
      LiteralModulePrivate = '(?i)src[/\\]powershell[/\\].*[/\\]Private[/\\]'
      DynamicModulePrivate = '(?i)Join-Path(?:\s+-Path)?\s+\$(?:module\w*|[A-Za-z_]\w*module\w*)\s+(?:-ChildPath\s+)?["'']Private(?:[/\\][^"'']*)?["'']'
      PathHelpers = '(?i)scripts[/\\]PathHelpers\.ps1'
      ModuleInvocation = '&\s*\$module\s*\{'
    }
    $mutationFixtures = @{
      LiteralModulePrivate = "Import-Module 'src/powershell/path/NetworkLantern.Path/Private/Hidden.ps1'"
      DynamicModulePrivate = "Join-Path `$moduleRoot 'Private/Hidden.ps1'"
      PathHelpers = "& 'scripts/PathHelpers.ps1'"
      ModuleInvocation = '& $module { Invoke-Hidden }'
    }
    foreach ($patternName in $prohibitedAdapterReferences.Keys) {
      $mutationFixtures[$patternName] | Should -Match $prohibitedAdapterReferences[$patternName] -Because "$patternName must catch its prohibited form"
    }

    foreach ($file in $adapterFiles) {
      $source = Get-Content -LiteralPath $file.FullName -Raw
      foreach ($patternName in $prohibitedAdapterReferences.Keys) {
        $source | Should -Not -Match $prohibitedAdapterReferences[$patternName] -Because "$($file.FullName) violates $patternName"
      }
    }
  }

  It 'keeps the throughput budget contract aligned across workflow and capability boundaries' {
    Import-Module (Join-Path $script:RepoRoot 'src/powershell/workflow/NetworkLantern.Workflow/NetworkLantern.Workflow.psd1') -Force
    Import-Module (Join-Path $script:RepoRoot 'src/powershell/throughput/NetworkLantern.Throughput.psd1') -Force
    $surfaces = @(
      @{ Command = (Join-Path $script:RepoRoot 'Invoke-NetworkLantern.ps1'); Parameter = 'ThroughputMaxTotalTests' }
      @{ Command = 'New-NetworkLanternWorkflowPlan'; Parameter = 'ThroughputMaxTotalTests' }
      @{ Command = (Join-Path $script:RepoRoot 'apps/throughput/Measure-NetworkThroughput.ps1'); Parameter = 'MaxTotalTests' }
      @{ Command = 'Measure-NetworkThroughput'; Parameter = 'MaxTotalTests' }
    )
    foreach ($surface in $surfaces) {
      $parameter = (Get-Command $surface.Command).Parameters[$surface.Parameter]
      $parameter.ParameterType | Should -Be ([int])
      $range = @($parameter.Attributes | Where-Object { $_ -is [System.Management.Automation.ValidateRangeAttribute] })
      $range.Count | Should -Be 1
      $range[0].MinRange | Should -Be 0
      $range[0].MaxRange | Should -Be 1000000
    }
  }

  It 'keeps capability modules independent from app and development-script layers' {
    $moduleFiles = Get-ChildItem -LiteralPath (Join-Path $script:RepoRoot 'src/powershell') -Recurse -File -Include '*.ps1', '*.psm1'
    $prohibitedModuleReferences = [ordered]@{
      AppPath = '(?i)(^|[/\\])apps[/\\]'
      QuotedRelativeAppPath = '(?i)["''](?:\.\.?[/\\])?apps[/\\]'
      ScriptPath = '(?i)(^|[/\\])scripts[/\\]'
      QuotedRelativeScriptPath = '(?i)["''](?:\.\.?[/\\])?scripts[/\\]'
    }
    $mutationFixtures = @{
      AppPath = 'C:/repository/apps/path/Test-NetworkPath.ps1'
      QuotedRelativeAppPath = "Join-Path `$repositoryRoot 'apps/path/Test-NetworkPath.ps1'"
      ScriptPath = 'C:/repository/scripts/ci.ps1'
      QuotedRelativeScriptPath = "Join-Path `$repositoryRoot './scripts/ci.ps1'"
    }
    foreach ($patternName in $prohibitedModuleReferences.Keys) {
      $mutationFixtures[$patternName] | Should -Match $prohibitedModuleReferences[$patternName] -Because "$patternName must catch its prohibited form"
    }

    foreach ($file in $moduleFiles) {
      $source = Get-Content -LiteralPath $file.FullName -Raw
      foreach ($patternName in $prohibitedModuleReferences.Keys) {
        $source | Should -Not -Match $prohibitedModuleReferences[$patternName] -Because "$($file.FullName) violates $patternName"
      }
    }
  }

  It 'keeps the workflow module as a non-executing plan builder' {
    $workflowRoot = Join-Path $script:RepoRoot 'src/powershell/workflow/NetworkLantern.Workflow'
    $workflowSource = (Get-ChildItem -LiteralPath $workflowRoot -Recurse -File -Include '*.ps1', '*.psm1' |
      Get-Content -Raw) -join "`n"

    $workflowSource | Should -Not -Match '(?im)^\s*exit\b'
    $workflowSource | Should -Not -Match 'RepositoryRoot'
    $workflowSource | Should -Not -Match 'Start-Process|EncodedCommand|NETWORK_LANTERN_CHILD'
  }

  It 'uses one explicit Bash feature loader from the public MTR adapter' {
    $entrypoint = Join-Path $script:RepoRoot 'apps/path/test-network-path.sh'
    $source = Get-Content -LiteralPath $entrypoint -Raw
    $bashFeatureRoot = Join-Path $script:RepoRoot 'src/bash/path'
    $loaderPath = Join-Path $bashFeatureRoot 'load.sh'
    $loaderSource = Get-Content -LiteralPath $loaderPath -Raw

    $source | Should -Match 'src/bash/path/load\.sh'
    $source | Should -Not -Match 'src/bash/path/lib/'
    @($source -split "`n").Count | Should -BeLessOrEqual 40

    $sourceStatementPattern = '(?m)^\s*(?:source|\.)\s+'
    'source "$PATH_FEATURE_DIR/lib/hidden.sh"' | Should -Match $sourceStatementPattern -Because 'the source scanner must catch a library composition statement'
    foreach ($library in Get-ChildItem -LiteralPath (Join-Path $bashFeatureRoot 'lib') -File -Filter '*.sh') {
      $loaderSource | Should -Match ([regex]::Escape("lib/$($library.Name)")) -Because 'load.sh must inventory every Bash library'
      (Get-Content -LiteralPath $library.FullName -Raw) | Should -Not -Match $sourceStatementPattern -Because "$($library.Name) must not compose another library"
    }
    $loaderSource | Should -Match ([regex]::Escape('main.sh'))
    foreach ($file in Get-ChildItem -LiteralPath $bashFeatureRoot -Recurse -File -Filter '*.sh') {
      if ($file.FullName -ne $loaderPath -and $file.Directory.Name -ne 'lib') {
        (Get-Content -LiteralPath $file.FullName -Raw) | Should -Not -Match $sourceStatementPattern -Because "$($file.Name) must not be another Bash composition root"
      }
    }
  }

  It 'marks exactly the directly invoked shell scripts executable in Git' {
    $expected = @(
      'apps/path/test-network-path.sh'
      'scripts/ci-legacy.sh'
      'scripts/ci-local.sh'
      'scripts/ci-rust.sh'
      'scripts/install-test-deps.sh'
      'scripts/prepare-macos-app.sh'
      'scripts/run-workflow.sh'
    )
    $executable = @(
      & git -C $script:RepoRoot ls-files --stage | ForEach-Object {
        if ($_ -match '^100755\s+\S+\s+\d+\s+(?<path>.+)$') { $Matches.path }
      }
    )
    $LASTEXITCODE | Should -Be 0

    ($executable | Sort-Object) -join "`n" | Should -Be (($expected | Sort-Object) -join "`n")
  }

  It 'keeps the static planner honest about its non-executing boundary' {
    $html = Get-Content -LiteralPath (Join-Path $script:RepoRoot 'site/index.html') -Raw
    $javascript = (Get-ChildItem -LiteralPath (Join-Path $script:RepoRoot 'site') -Filter '*.js' -File | Get-Content -Raw) -join "`n"

    $html | Should -Match '(?i)(no commands run|makes no network probes)'
    $html | Should -Not -Match '(?i)<form\b'
    $javascript | Should -Match 'network-lantern workflow'
    $javascript | Should -Not -Match 'Invoke-NetworkLantern\.ps1'
    $javascript | Should -Not -Match '(?i)\b(fetch|WebSocket|EventSource|sendBeacon|XMLHttpRequest|indexedDB|localStorage|sessionStorage|serviceWorker)\b'
  }
}
