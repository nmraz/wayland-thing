project('wayland-thing', 'c')

wayland_client = dependency('wayland-client')

defines = ['-D_GNU_SOURCE']

sources = [
    'src/main.c',
    'src/protocols/xdg-shell.c',
]

executable(
    'wayland-thing',
    sources,
    dependencies: [wayland_client],
    c_args: defines,
)
