project('wayland-thing', 'c')

wayland_client = dependency('wayland-client')

defines = ['-D_GNU_SOURCE']

executable(
    'wayland-thing',
    'src/main.c',
    dependencies: [wayland_client],
    c_args: defines,
)
