# baud-present-vulkan

baud's Vulkan presentation on Linux: a frame drawn into an exported dmabuf and shown through `baud-present-wayland`. Every choice (device, format and modifier, sync) is made from capabilities queried at run time; anything missing leaves the window on shared memory.
