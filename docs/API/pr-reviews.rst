Pull Request Reviews
====================

.. autoclass:: git_bot_feedback.ReviewOptions
    :members:

.. autoclass:: git_bot_feedback.ReviewAction

    .. py:attribute:: ReviewAction.Approve

        A review that approves the changes.

    .. py:attribute:: ReviewAction.RequestChanges

        A review that requests changes to the code.

    .. py:attribute:: ReviewAction.Comment

        A review that comments on the code without approving or requesting changes.

.. autoclass:: git_bot_feedback.ReviewComment
    :members:
