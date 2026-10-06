"""
This example demonstrates how to use the `git-bot-feedback` package to create log groups.

The supported CI platforms expect a special format for log-grouping statements.
Note, the git-bot-feedback package will automatically format the log-grouping statements
accordingly, but logging API in Python may prefix the log message with unwanted
characters (like the log level or timestamp).

This example shows how to set up logging in Python to ensure that log-grouping messages are
formatted correctly.
"""

import logging
import sys


def setup_logging():
    # config for global logging, this will be applied to all loggers in the application
    logging.basicConfig(level=logging.INFO)

    # most CI platforms expect no additional prefixes or suffixes in log-grouping statements.
    ci_log_fmt = logging.Formatter("%(message)s")

    # direct CI log messages to stdout.
    ci_log_handler = logging.StreamHandler(sys.stdout)
    # activate the custom formatter for the CI log handler
    ci_log_handler.setFormatter(ci_log_fmt)

    # get the logger used for CI log grouping.
    ci_logger = logging.getLogger("CI_LOG_GROUPING")
    # activate the custom handler/formatter for the CI logger
    ci_logger.addHandler(ci_log_handler)

    # DO NOT PROPAGATE CI_LOG_GROUPING logs to the root logger (prevents duplicate logs).
    ci_logger.propagate = False


def main():
    # need to setup logging in python first.
    setup_logging()

    # upon import (module init), the logging config is adopted.
    import git_bot_feedback as gfb  # type: ignore

    # instantiate CI platform support (can also be run locally outside of CI).
    client = gfb.GitClient()

    # The output of these functions are tailored to the CI platform detected in `gfb.GitClient()`.
    client.start_log_group("group name")
    client.end_log_group("group name")


if __name__ == "__main__":
    main()
