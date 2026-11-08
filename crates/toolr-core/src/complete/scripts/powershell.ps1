# toolr PowerShell completion.
if ($PSVersionTable.PSVersion -lt [version]'5.1') {
    Write-Warning 'toolr completion needs PowerShell 5.1 or newer'
} else {
    function global:_toolr_complete($wordToComplete, $commandAst, $cursorPosition) {
    }
    Register-ArgumentCompleter -Native -CommandName 'toolr', 'toolr.exe' -ScriptBlock ${function:_toolr_complete}
}
