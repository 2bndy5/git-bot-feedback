# GitLab Token Permissions

This is an exhaustive list of the permissions needed by the GitLab client, organized by feature.
Each REST API endpoint that the client uses is listed with the permission it needs.

The client reads the token from the `GITLAB_TOKEN` environment variable.
Store the token as a [masked CI/CD variable][masked-vars] in the project's settings.
The CI/CD job token (`CI_JOB_TOKEN`) is not used.

Every request must pass two checks:

1. The token has permission to call the endpoint.
2. The token's user has a [role][roles] that allows the action.
   For example, resolving a thread needs the Developer role
   even if the token has permission to call the endpoint.

The `:id` in each path is the project of the merge request.
For a merge request from a fork, the pipeline runs in the fork, but `:id` is the
target project (`CI_MERGE_REQUEST_PROJECT_ID`). So the token needs access to the target project.

[masked-vars]: https://docs.gitlab.com/ci/variables/#mask-a-cicd-variable
[roles]: https://docs.gitlab.com/user/permissions/

## Token types

| Token | How to grant access |
| ----- | ------------------- |
| [Fine-grained personal access token][fine-grained] (recommended, GitLab v19.2 or newer[^beta]) | Under **Group and project access**, include the project. Then add the resource permissions listed below. |
| Legacy personal, [project][project-tokens], or [group][group-tokens] access token | The `api` scope. The `read_api` scope is enough if only [file changes](#file-changes) are needed. |

Fine-grained permissions are only available for personal access tokens.
Project and group access tokens still use scopes.
For a project or group access token, also select a role for its bot user;
see [user roles](#user-roles).

[^beta]: Fine-grained personal access tokens were available as a beta in GitLab v18.10 and v19.1.

[fine-grained]: https://docs.gitlab.com/auth/tokens/fine_grained_access_tokens/
[project-tokens]: https://docs.gitlab.com/user/project/settings/project_access_tokens/
[group-tokens]: https://docs.gitlab.com/user/group/settings/group_access_tokens/

## Summary

All features together need these fine-grained permissions:

| Resource | Permissions | Used by |
| -------- | ----------- | ------- |
| Merge Request | Read, Update, Approve; Delete (optional) | [file changes](#file-changes), [thread comments](#thread-comments), [merge request reviews](#merge-request-reviews) |
| Work Item | Read, Create, Update, Delete | [thread comments](#thread-comments), [merge request reviews](#merge-request-reviews) |
| Commit | Read | [file changes](#file-changes) in push pipelines |

> [!NOTE]
> GitLab lists a merge request's comments (notes) under the **Work Item** resource,
> not the **Merge Request** resource.
> But a merge request's threads (discussions) and draft notes are under the **Merge Request** resource.

## File changes

To list the files changed in a pipeline:

| Pipeline | Endpoint | Permission |
| -------- | -------- | ---------- |
| Merge request | `GET /projects/:id/merge_requests/:merge_request_iid/raw_diffs` | Merge Request: Read |
| Push (or other) | `GET /projects/:id/repository/commits/:sha/diff` | Commit: Read |

If the project is public, these endpoints can be read without the token having these permissions.
See GitLab's [publicly accessible endpoints][public-endpoints].

[public-endpoints]: https://docs.gitlab.com/auth/tokens/fine_grained_access_tokens_rest/#publicly-accessible-endpoints

## Thread comments

Thread comments are only posted in merge request pipelines.
In push pipelines, no request is made.

| Endpoint | Permission | Why |
| -------- | ---------- | --- |
| `GET /projects/:id/merge_requests/:merge_request_iid` | Merge Request: Read | Check if the merge request is locked |
| `GET /projects/:id/merge_requests/:noteable_id/notes` | Work Item: Read | Find comments previously posted |
| `POST /projects/:id/merge_requests/:noteable_id/notes` | Work Item: Create | Post a new comment |
| `PUT /projects/:id/merge_requests/:noteable_id/notes/:note_id` | Work Item: Update | Update the previous comment ([`CommentPolicy::Update`]) |
| `DELETE /projects/:id/merge_requests/:noteable_id/notes/:note_id` | Work Item: Delete | Delete outdated comments |

[`CommentPolicy::Update`]: https://docs.rs/git-bot-feedback/latest/git_bot_feedback/enum.CommentPolicy.html

## Merge request reviews

A review is posted like GitLab's web UI submits one:
each comment is created as a draft note, then the draft notes are published.

| Endpoint | Permission | Why |
| -------- | ---------- | --- |
| `GET /projects/:id/merge_requests/:merge_request_iid` | Merge Request: Read | Check if the merge request is a draft, closed, or locked |
| `GET /projects/:id/merge_requests/:noteable_id/discussions` | Merge Request: Read | Find review comments previously posted |
| `PUT /projects/:id/merge_requests/:noteable_id/discussions/:discussion_id` | Merge Request: Update | Resolve outdated review comments |
| `DELETE /projects/:id/merge_requests/:noteable_id/discussions/:discussion_id/notes/:note_id` | Merge Request: Delete | Delete outdated review comments (only if [`ReviewOptions::delete_review_comments`] is `true`) |
| `GET /projects/:id/merge_requests/:merge_request_iid/draft_notes` | Merge Request: Read | Find pending draft notes |
| `DELETE /projects/:id/merge_requests/:merge_request_iid/draft_notes/:draft_note_id` | Merge Request: Update | Delete draft notes left by a review that failed to be published |
| `GET /projects/:id/merge_requests/:merge_request_iid/raw_diffs` | Merge Request: Read | Map each comment to a line in the diff |
| `POST /projects/:id/merge_requests/:merge_request_iid/draft_notes` | Merge Request: Update | Create review comments as draft notes |
| `POST /projects/:id/merge_requests/:merge_request_iid/draft_notes/bulk_publish` | Merge Request: Update | Publish the review |
| `PUT /projects/:id/merge_requests/:merge_request_iid/draft_notes/:draft_note_id/publish` | Merge Request: Update | Publish each review comment (only if the token's user has other pending draft notes) |
| `POST /projects/:id/merge_requests/:noteable_id/notes` | Work Item: Create | Post the review summary |
| `POST /projects/:id/merge_requests/:merge_request_iid/approve` | Merge Request: Approve | Approve the merge request ([`ReviewAction::Approve`]) |
| `POST /projects/:id/merge_requests/:merge_request_iid/unapprove` | Merge Request: Approve | Withdraw a previous approval ([`ReviewAction::RequestChanges`]) |

Deleting a draft note needs the **Update** permission, not **Delete**.
The **Delete** permission is only needed if [`ReviewOptions::delete_review_comments`] is `true`.

[`ReviewOptions::delete_review_comments`]: https://docs.rs/git-bot-feedback/latest/git_bot_feedback/struct.ReviewOptions.html#structfield.delete_review_comments
[`ReviewAction::Approve`]: https://docs.rs/git-bot-feedback/latest/git_bot_feedback/enum.ReviewAction.html#variant.Approve
[`ReviewAction::RequestChanges`]: https://docs.rs/git-bot-feedback/latest/git_bot_feedback/enum.ReviewAction.html#variant.RequestChanges

## User roles

The token's user also needs a role in the project:

| Feature | Minimum role |
| ------- | ------------ |
| [File changes](#file-changes) | Reporter (Guest is enough for a public project) |
| [Thread comments](#thread-comments) | Reporter ([comment on merge requests][mr-permissions]) |
| [Merge request reviews](#merge-request-reviews) | Developer ([resolve threads][resolve-threads]) |
| Approving a merge request | An [eligible approver][eligible-approvers] (Developer by default) |

[mr-permissions]: https://docs.gitlab.com/user/permissions/#merge-request-permissions
[resolve-threads]: https://docs.gitlab.com/api/discussions/#resolve-a-merge-request-thread
[eligible-approvers]: https://docs.gitlab.com/user/project/merge_requests/approvals/rules/#eligible-approvers

## References

- [Fine-grained permissions for REST API](https://docs.gitlab.com/auth/tokens/fine_grained_access_tokens_rest/)
- [GitLab REST API reference](https://api.gitlab.com/rest/#gitlab)
