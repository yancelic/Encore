# Encore's background recorder only needs native window/audio capture and encoders.
cmake_minimum_required(VERSION 3.28)
foreach(plugin linux-capture linux-pipewire linux-pulseaudio obs-ffmpeg obs-nvenc obs-x264)
  add_subdirectory(${plugin})
endforeach()
