project('wayland-thing', 'c')

cc = meson.get_compiler('c')

libwayland_client = dependency('wayland-client')
libm = cc.find_library('m')

defines = ['-D_GNU_SOURCE']

sources = [
    'src/main.c',
    'src/protocols/xdg-shell.c',
]

executable(
    'wayland-thing',
    sources,
    dependencies: [libwayland_client, libm],
    c_args: defines,
)
