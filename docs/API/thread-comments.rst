Thread Comments
===============

.. autoclass:: git_bot_feedback.ThreadCommentOptions
    :members:

.. autoclass:: git_bot_feedback.CommentKind

    .. py:attribute:: CommentKind.Lgtm

        A comment that indicates that the code is good to go.
    
    .. py:attribute:: CommentKind.Concerns

        A comment that indicates that there are concerns with the code.

.. autoclass:: git_bot_feedback.CommentPolicy
    
    .. py:attribute:: CommentPolicy.Anew

        Always post a new comment.
        Old comments (from the same bot/author) will be deleted.

    .. py:attribute:: CommentPolicy.Update

        Update any existing comment or post a new one if no previous comments found.

        Again, comments in scope are authored bny the same bot/author.
        If any more than 1 comment of the same bot/author is found,
        then all but the most recent comment will be deleted.
