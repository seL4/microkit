# apparently, order is quite important, do not reorder statements!
cmake_minimum_required(VERSION 3.7.2)

include(CMakePrintHelpers)

list(
    APPEND
        CMAKE_MODULE_PATH
        "${CMAKE_CURRENT_LIST_DIR}/kernel"
        "${CMAKE_CURRENT_LIST_DIR}/capdl"
        "${CMAKE_CURRENT_LIST_DIR}/seL4_tools/cmake-tool/helpers"
        "${CMAKE_CURRENT_LIST_DIR}/musllibc"
        "${CMAKE_CURRENT_LIST_DIR}/sel4runtime"
        "${CMAKE_CURRENT_LIST_DIR}/seL4_libs"
        "${CMAKE_CURRENT_LIST_DIR}/util_libs"
        "${CMAKE_CURRENT_LIST_DIR}/seL4_tools/elfloader-tool"
)

set(SEL4CP_BUILD_DIR "${CMAKE_CURRENT_SOURCE_DIR}/tmp_build/hello-goodbye")
set(CDL_TARGET "hello.cdl")

if("${PLATFORM}" STREQUAL "")
  set(PLATFORM "zynqmp")
endif()
# FIXME: use non mcs kernel for case initializer
set(KernelIsMCS ON)
set(CROSS_COMPILER_PREFIX "aarch64-none-elf-")
# zf_log_debug
# set(LibUtilsDefaultZfLogLevel 2)
set(CapDLLoaderMaxObjects 20000)
set(KernelRootCNodeSizeBits 17)

include(application_settings)
correct_platform_strings()
find_package(seL4 REQUIRED)
sel4_configure_platform_settings()
ApplyCommonReleaseVerificationSettings(FALSE FALSE)

sel4_import_kernel()
if(KernelArchARM)
    ApplyData61ElfLoaderSettings(${KernelPlatform} ${KernelSel4Arch})
endif()
# zynqmp should be using elf in newer version of ApplyData61ElfLoaderSettings
set(ElfloaderImage "elf" CACHE STRING "" FORCE)
find_package(elfloader-tool REQUIRED)
elfloader_import_project()

project(hello-capdl C ASM)

find_package(musllibc REQUIRED)
find_package(util_libs REQUIRED)
find_package(seL4_libs REQUIRED)
find_package(capdl REQUIRED)

sel4_import_libsel4()
util_libs_import_libraries()
sel4_libs_import_libraries()
musllibc_setup_build_environment_with_sel4runtime()

CapDLToolInstall(install_capdl_tool CAPDL_TOOL_BINARY)
capdl_import_project()
set(CapDLStaticAlloc OFF)
file(COPY "${SEL4CP_BUILD_DIR}/${CDL_TARGET}" DESTINATION "${CMAKE_CURRENT_BINARY_DIR}")
CapDLToolCFileGen(capdl_c_spec_target capdl_spec.c ${CapDLStaticAlloc} "$<TARGET_PROPERTY:object_sizes,FILE_PATH>" "${CDL_TARGET}" "${CAPDL_TOOL_BINARY}"
    DEPENDS install_capdl_tool "${CAPDL_TOOL_BINARY}")

file(GLOB CDL_ELF_SOURCES CONFIGURE_DEPENDS "${SEL4CP_BUILD_DIR}/*.elf")
file(COPY ${CDL_ELF_SOURCES} DESTINATION "${CMAKE_CURRENT_BINARY_DIR}" FILES_MATCHING PATTERN "*.elf")
file(GLOB CDL_ELFS CONFIGURE_DEPENDS "${CMAKE_CURRENT_BINARY_DIR}/*.elf")

BuildCapDLApplication(C_SPEC capdl_spec.c
ELF ${CDL_ELFS}
DEPENDS capdl_c_spec_target
OUTPUT capdl-loader)

include(rootserver)
DeclareRootserver(capdl-loader)

if("${PLATFORM}" STREQUAL "qemu-arm-virt")
  include(simulation)
  GenerateSimulateScript()
endif()
