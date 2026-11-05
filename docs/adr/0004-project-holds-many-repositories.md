# A Project holds many Repositories from the start

gikli opens a Project, which is a folder holding one or more Repositories, not a single repository. The model supports several Repositories even though multi-repository support is Scope 3 and the MVP works with one. Changelists can span Repositories, as they do in JetBrains, and "Open repository" in Scope 1 already detects nested roots that are not registered yet. Retrofitting multiple roots later would mean reworking changelists and every view.
