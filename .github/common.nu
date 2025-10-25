
export def --wrapped run-cmd [...cmd: string] {
    let app = if ($cmd | first) == "cargo" {
        ($cmd | first 2) | str join ' '
    } else {
        ($cmd | first)
    }
    print $"(ansi blue)\nRunning(ansi reset) ($cmd | str join ' ')"
    let elapsed = timeit {|| ^($cmd | first) ...($cmd | skip 1)}
    print $"(ansi magenta)($app) took ($elapsed)(ansi reset)"
}
