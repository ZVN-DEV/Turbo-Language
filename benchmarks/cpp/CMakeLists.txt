cmake_minimum_required(VERSION 3.10)
project(cpp_benchmarks LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Release-level optimisation by default
if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release)
endif()

set(CMAKE_CXX_FLAGS_RELEASE "-O2")

add_executable(benchmark main.cpp)

# Link pthreads (required for std::thread on most platforms)
find_package(Threads REQUIRED)
target_link_libraries(benchmark PRIVATE Threads::Threads)
