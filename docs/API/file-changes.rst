File Changes
============

.. autoclass:: git_bot_feedback.FileFilter
    :members:

.. autoclass:: git_bot_feedback.DiffHunkHeader
    :members:

.. autoclass:: git_bot_feedback.FileDiffLines
    :members:

.. autoclass:: git_bot_feedback.LinesChangedOnly

    .. py:class:: git_bot_feedback.LinesChangedOnly.On

        Only the lines in the diff that have additions.
    
    .. py:class:: git_bot_feedback.LinesChangedOnly.Off

        All lines in the file, regardless of the diff contents.

    .. py:class:: git_bot_feedback.LinesChangedOnly.Diff

        All lines in the diff, including context lines.

.. autofunction:: git_bot_feedback.parse_diff
