# PSScriptAnalyzer settings: exclude rules that are not practical for this repo.
# - PSAvoidGlobalVars: test file uses global for InModuleScope fixture visibility.
# - PSUseShouldProcessForStateChangingFunctions: New-* test/module helpers return in-memory objects only.
# - PSUseSingularNouns: Get-BitsPerSecondMbps uses standard unit "Mbps"; Get-NetworkThroughputDefaultParameterSet is singular.
# - PSUseOutputTypeCorrectly: utility conversion helpers intentionally return heterogeneous scalar/array types.
@{
  Severity = @('Error', 'Warning')
  ExcludeRules = @(
    'PSAvoidGlobalVars',
    'PSUseBOMForUnicodeEncodedFile',
    'PSUseShouldProcessForStateChangingFunctions',
    'PSUseSingularNouns',
    'PSUseOutputTypeCorrectly'
  )
}
