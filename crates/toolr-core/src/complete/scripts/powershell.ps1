# toolr PowerShell completion - delegates to `toolr __complete`.
# Needs Windows PowerShell 5.1 or PowerShell 7.
#
# Install via `toolr self completion install powershell` and dot-source the file it writes from
# your profile, or load it on every shell start with:
#   Invoke-Expression (& toolr self completion print powershell | Out-String)
#
# Known limits: when toolr has no candidates PowerShell falls back to its own file-name
# completion; a cursor inside a word completes the whole word; a word holding a variable,
# sub-expression or array (`$x`, `"a$(x)"`, `a,b`) completes nothing, nor does anything after
# one; in a file-name partial `*`, `?` and `[...]` are wildcards, as in PowerShell's own
# completion, so a partial holding a whole `[...]` pair matches nothing; a toolr reached only
# through a .cmd/.bat shim completes nothing; PowerShell ISE isn't supported; Windows
# PowerShell 5.1 never calls this completer for a bare `-` or `--`, or a word like `-x`
# (type a letter after `--`).
#
# The script stays pure ASCII (5.1 reads a BOM-less file in the ANSI code page) and parses on
# PowerShell 3, so the version guard below can run there.

if ($PSVersionTable.PSVersion -lt [version]'5.1') {
    Write-Warning 'toolr completion needs PowerShell 5.1 or newer'
} else {
    # One argument in the MSVC rules .NET and Rust's std::env::args both parse. Legacy
    # native-argument passing (5.1, pwsh before 7.3) drops empty words and mangles `"`.
    function global:_toolr_msvc_arg([string]$Word) {
        if ($Word.Length -gt 0 -and $Word -notmatch '[\s"]') {
            return $Word
        }
        $bsl = [char]92
        $sb = New-Object System.Text.StringBuilder
        [void]$sb.Append('"')
        $run = 0
        foreach ($c in $Word.ToCharArray()) {
            if ($c -eq $bsl) {
                $run++
                continue
            }
            if ($c -eq [char]34) {
                [void]$sb.Append($bsl, 2 * $run + 1)
            } elseif ($run -gt 0) {
                [void]$sb.Append($bsl, $run)
            }
            [void]$sb.Append($c)
            $run = 0
        }
        if ($run -gt 0) {
            [void]$sb.Append($bsl, 2 * $run)
        }
        [void]$sb.Append('"')
        return $sb.ToString()
    }

    # The text PowerShell passes a native command for one command element, or $null when only
    # running the line would tell (variables, sub-expressions, arrays). Nothing is evaluated.
    function global:_toolr_word_value($Ast) {
        if ($Ast -is [System.Management.Automation.Language.StringConstantExpressionAst]) {
            return $Ast.Value
        }
        if ($Ast -is [System.Management.Automation.Language.ExpandableStringExpressionAst]) {
            if ($Ast.NestedExpressions.Count -eq 0) {
                return $Ast.Value
            }
            return $null
        }
        # `--` and `-x:y` parse as parameters; numbers as constants. Natives get their text.
        if ($Ast -is [System.Management.Automation.Language.CommandParameterAst] -or
            $Ast -is [System.Management.Automation.Language.ConstantExpressionAst]) {
            return $Ast.Extent.Text
        }
        return $null
    }

    # What `$Text` would pass as the single argument of a native command, or $null.
    function global:_toolr_parse_word([string]$Text) {
        $tokens = $null
        $errors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseInput("x $Text", [ref]$tokens, [ref]$errors)
        if ($errors.Count -gt 0 -or $ast.EndBlock.Statements.Count -ne 1) {
            return $null
        }
        $cmd = $ast.EndBlock.Statements[0].PipelineElements
        if ($cmd.Count -ne 1) {
            return $null
        }
        $elements = $cmd[0].CommandElements
        if ($null -eq $elements -or $elements.Count -ne 2) {
            return $null
        }
        return _toolr_word_value $elements[1]
    }

    # `$Value` as one word the parser reads back exactly. `$Open` is the quote the user typed.
    function global:_toolr_quote([string]$Value, [string]$Open) {
        # pwsh 7.4+ tilde-expands a bare leading `~` on Unix; the parser check rejects numbers.
        if ($Value -match '^[A-Za-z0-9_./\\:=+-]+$' -and -not $Value.StartsWith('~') -and
            (_toolr_parse_word $Value) -ceq $Value) {
            return $Value
        }
        $doubles = '"' + [char]0x201C + [char]0x201D + [char]0x201E
        if ($Open.Length -gt 0 -and $doubles.Contains($Open)) {
            $sb = New-Object System.Text.StringBuilder
            [void]$sb.Append('"')
            foreach ($c in $Value.ToCharArray()) {
                if (('`$' + $doubles).Contains([string]$c)) {
                    [void]$sb.Append('`')
                }
                [void]$sb.Append($c)
            }
            [void]$sb.Append('"')
            return $sb.ToString()
        }
        $singles = "'" + [char]0x2018 + [char]0x2019 + [char]0x201A + [char]0x201B
        $sb = New-Object System.Text.StringBuilder
        [void]$sb.Append("'")
        foreach ($c in $Value.ToCharArray()) {
            if ($singles.Contains([string]$c)) {
                [void]$sb.Append($c)
            }
            [void]$sb.Append($c)
        }
        [void]$sb.Append("'")
        return $sb.ToString()
    }

    # The toolr executable, skipping functions and aliases (a wrapper could recurse) and, on
    # Windows, .cmd/.bat shims, which cmd.exe would re-parse.
    function global:_toolr_exe {
        $windows = [System.IO.Path]::DirectorySeparatorChar -eq [char]92
        foreach ($c in @(Get-Command -Name toolr -CommandType Application -ErrorAction SilentlyContinue)) {
            if (-not $windows -or [System.IO.Path]::GetExtension($c.Path) -eq '.exe') {
                return $c.Path
            }
        }
        return $null
    }

    # Run `toolr __complete`. A Process, not `&`: see _toolr_msvc_arg, and `&` decodes stdout
    # with the console code page.
    function global:_toolr_run([string]$Exe, [string]$Cwd, [string[]]$Words) {
        $argv = @('__complete', $Cwd) + $Words
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = $Exe
        $psi.Arguments = (@($argv | ForEach-Object { _toolr_msvc_arg $_ }) -join ' ')
        $psi.UseShellExecute = $false
        $psi.CreateNoWindow = $true
        $psi.WorkingDirectory = $Cwd
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $psi.StandardOutputEncoding = New-Object System.Text.UTF8Encoding $false
        $proc = [System.Diagnostics.Process]::Start($psi)
        # Drain stderr concurrently so neither pipe can fill and block the child.
        $stderr = $proc.StandardError.ReadToEndAsync()
        $stdout = $proc.StandardOutput.ReadToEnd()
        $proc.WaitForExit()
        [void]$stderr.Wait(1000)
        return @{ Code = $proc.ExitCode; Out = $stdout }
    }

    function global:_toolr_complete($wordToComplete, $commandAst, $cursorPosition) {
        # Function-local, so a strict profile can't turn a quiet completion into an error.
        $ErrorActionPreference = 'Continue'
        $PSNativeCommandUseErrorActionPreference = $false
        Set-StrictMode -Off
        try {
            $location = Get-Location
            # toolr can only search a real directory for tools/.
            if ($location.Provider.Name -ne 'FileSystem') {
                return
            }
            $cwd = $location.ProviderPath
            $exe = _toolr_exe
            if (-not $exe) {
                return
            }
            # $wordToComplete closes an open quote and keeps the quoting, so read the AST.
            $words = New-Object System.Collections.Generic.List[string]
            $current = $null
            $elements = $commandAst.CommandElements
            for ($i = 1; $i -lt $elements.Count; $i++) {
                $element = $elements[$i]
                if ($element.Extent.EndOffset -lt $cursorPosition) {
                    $word = _toolr_word_value $element
                    if ($null -eq $word -or $element.Extent.Text -eq '--%') {
                        return
                    }
                    $words.Add($word)
                } else {
                    if ($element.Extent.StartOffset -lt $cursorPosition) {
                        $current = $element
                    }
                    break
                }
            }
            $raw = ''
            $value = ''
            if ($null -ne $current) {
                $raw = $current.Extent.Text
                $value = _toolr_word_value $current
                if ($null -eq $value) {
                    return
                }
            }
            $words.Add($value)

            # `--opt=partial`: the engine returns bare values, so put `--opt=` back.
            $prefix = ''
            if ($value -match '^(--[^=]+=)') {
                $prefix = $Matches[1]
            }
            $afterPrefix = $raw
            if ($prefix.Length -gt 0 -and $raw.StartsWith($prefix)) {
                $afterPrefix = $raw.Substring($prefix.Length)
            }
            $open = ''
            if ($raw.Length -gt 0) {
                $open = $raw.Substring(0, 1)
                if ($open -notmatch '[''"]' -and $afterPrefix.Length -gt 0) {
                    $open = $afterPrefix.Substring(0, 1)
                }
            }

            $reply = _toolr_run $exe $cwd $words.ToArray()
            if ($reply.Code -eq 0) {
                foreach ($line in ($reply.Out -split "`n")) {
                    $line = $line.TrimEnd("`r")
                    if ($line.Length -eq 0) {
                        continue
                    }
                    $parts = $line.Split([char]9, 2)
                    $full = $prefix + $parts[0]
                    $tip = $full
                    if ($parts.Count -gt 1 -and $parts[1].Length -gt 0) {
                        $tip = $parts[1]
                    }
                    New-Object System.Management.Automation.CompletionResult (_toolr_quote $full $open), $full, 'ParameterValue', $tip
                }
            } elseif ($reply.Code -eq 10 -or $reply.Code -eq 11) {
                # The partial stays a wildcard pattern (`*` globs), as in PowerShell's own
                # native-command completion; escaping it would match nothing at all.
                $fileWord = _toolr_quote $value.Substring($prefix.Length) "'"
                $fsHome = (Get-PSProvider FileSystem).Home
                foreach ($result in [System.Management.Automation.CompletionCompleters]::CompleteFilename($fileWord)) {
                    if ($reply.Code -eq 11 -and $result.ResultType -ne 'ProviderContainer') {
                        continue
                    }
                    $path = _toolr_parse_word $result.CompletionText
                    if ($null -ne $path) {
                        # The text is escaped for -Path (`a`[1`]`); a native command needs the name.
                        # Only unescape what round-trips, so an undoubled backtick is never lost.
                        $name = [System.Management.Automation.WildcardPattern]::Unescape($path)
                        if ([System.Management.Automation.WildcardPattern]::Escape($name) -ceq $path) {
                            $path = $name
                        }
                        # The reply is quoted and PowerShell doesn't expand a quoted `~`.
                        if ($fsHome -and $path -match '^~([/\\]|$)') {
                            $path = $fsHome + $path.Substring(1)
                        }
                        $full = $prefix + $path
                        New-Object System.Management.Automation.CompletionResult (_toolr_quote $full $open), $result.ListItemText, $result.ResultType, $result.ToolTip
                    }
                }
            }
        } catch {
            return
        }
    }

    Register-ArgumentCompleter -Native -CommandName 'toolr', 'toolr.exe' -ScriptBlock ${function:_toolr_complete}
}
