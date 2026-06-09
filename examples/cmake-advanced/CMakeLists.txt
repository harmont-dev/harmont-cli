cmake_minimum_required(VERSION 3.20)
project(harmont_example_cmake_advanced CXX)

option(BUILD_TESTING "Build tests" ON)

add_library(mathlib src/math.cpp)
target_include_directories(mathlib PUBLIC src)

if(BUILD_TESTING)
  enable_testing()
  add_executable(test_math tests/test_math.cpp)
  target_link_libraries(test_math PRIVATE mathlib)
  add_test(NAME test_math COMMAND test_math)
endif()
