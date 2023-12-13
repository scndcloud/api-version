# Rust CI Templates for Github Workflows

## Example usage

Instead of simply copying into your own project, instead add this repository as
a remote and merge changes locally. This allows you to inherit not only the git
history of this project but also fetch future changes made centrally to this
repo. Be sure to add `--allow-unrelated` when you merge changes as Git rejects
git histories being merged from multiple repos by default.

```
git remote add ci-conf git@github.com:scndcloud/rust-ci-conf.git
git fetch ci-conf
git merge --allow-unrelated-histories ci-conf/main
```
