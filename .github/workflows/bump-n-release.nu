# This script automates the release process for all of the packages in this repository.
# In order, this script does the following:
#
# 1. Bump version number in Cargo.toml manifest.
#
#    This step requires `cargo-edit` installed.
#
# 2. Updates the CHANGELOG.md
#
#    Requires `git-cliff` (see https://git-cliff.org) to be installed
#    to regenerate the change logs from git history.
#
#    NOTE: `git cliff` uses GITHUB_TOKEN env var to access GitHub's REST API for
#    fetching certain data (like PR labels and commit author's username).
#
# 3. Pushes the changes from (steps 1 and 2) to remote
#
# 4. Creates a GitHub Release and uses the section from the CHANGELOG about the new tag
#    as a release description.
#
#    Requires `gh-cli` (see https://cli.github.com) to be installed to create the release
#    and push the tag.
#
#    NOTE: This step also tags the commit from step 3.
#    When a tag is pushed to the remote, the CI builds are triggered and
#    a package are published to crates.io
#
#    NOTE: In a CI run, the GITHUB_TOKEN env var to authenticate access.
#    Locally, you can use `gh login` to interactively authenticate the user account.
use ../common.nu run-cmd

export def is-in-ci [] {
    $env | get --optional CI | default "false" | ($in == "true") or ($in == true)
}

# Bump the version per the given component name (major, minor, patch)
export def bump-version [
    component: string # the version component to bump
    --dry-run, # do not actually write changes to disk
] {
    mut args = [--bump $component]
    if ($dry_run) {
        $args = $args | append "--dry-run"
    }
    let result = (
        (^cargo set-version ...$args)
        | complete
        | get stderr
        | lines
        | first
        | str trim
        | parse "Upgrading {pkg} from {old} to {new}"
        | first
    )
    print $"bumped ($result | get old) to ($result | get new)"
    $result | get new
}

# Use `git-cliff` tp generate changes.
#
# If `--unreleased` is asserted, then the `git-cliff` output will be saved to .config/ReleaseNotes.md.
# Otherwise, the generated changes will span the entire git history and be saved to CHANGELOG.md.
export def gen-changes [
    tag: string, # the new version tag to use for unreleased changes.
    --unreleased, # only generate changes from unreleased version.
] {
    mut args = [--tag, $tag, --config, .config/cliff.toml]
    let prompt = if $unreleased {
        let out_path = ".config/ReleaseNotes.md"
        $args = $args | append [--strip, header, --unreleased, --output, $out_path]
        {out_path: $out_path, log_prefix: "Generated"}
    } else {
        let out_path = "CHANGELOG.md"
        $args = $args | append [--output, $out_path]
        {out_path: $out_path, log_prefix: "Updated"}
    }
    run-cmd git-cliff ...$args
    print ($prompt | format pattern "{log_prefix} {out_path}")
}

# Is the the default branch currently checked out?
export def is-on-main [] {
    let branch = (
        ^git branch
        | lines
        | where {$in | str starts-with '*'}
        | first
        | str trim --left --char '*'
        | str trim
    ) == "main"
    $branch
}

export def main [component: string] {
    let is_ci = is-in-ci
    let ver = if $is_ci {
        bump-version --dry-run $component
    } else {
        bump-version $component
    }
    let tag = $"v($ver)"
    gen-changes $tag
    gen-changes $tag --unreleased
    let is_main = is-on-main
    if not $is_main {
        print $"(ansi yellow)Not checked out on default branch!(ansi reset)"
    }
    if $is_ci and $is_main {
        run-cmd git config --global user.name $"($env.GITHUB_ACTOR)"
        run-cmd git config --global user.email $"($env.GITHUB_ACTOR_ID)+($env.GITHUB_ACTOR)@users.noreply.github.com"
        run-cmd git add --all
        run-cmd git commit -m $"build: bump version to ($tag)"
        run-cmd git push
        print $"Deploying ($tag)"
        run-cmd gh release create $tag --notes-file ".config/ReleaseNotes.md"
    } else if $is_main {
        print $"(ansi yellow)Not deploying from local clone.(ansi reset)"
    }
}
