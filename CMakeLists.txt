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

set(PLATFORM "zynqmp")
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
# FIXME: don't hardcode cdl file. maybe detect from some dir?
set(CDL_TARGET "${CMAKE_CURRENT_BINARY_DIR}/hello.cdl")
add_custom_target(cdl_target DEPENDS "${CDL_TARGET}")
# from https://stackoverflow.com/a/55184207
add_custom_command(OUTPUT "${CDL_TARGET}"
                   COMMAND ${CMAKE_COMMAND} -E copy ${CMAKE_CURRENT_SOURCE_DIR}/tmp_build/hello-goodbye/hello.cdl
                                                    "${CDL_TARGET}"
                   DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/tmp_build/hello-goodbye/hello.cdl)
CapDLToolCFileGen(capdl_c_spec_target capdl_spec.c ${CapDLStaticAlloc} "$<TARGET_PROPERTY:object_sizes,FILE_PATH>" "${CDL_TARGET}" "${CAPDL_TOOL_BINARY}"
    DEPENDS cdl_target install_capdl_tool "${CAPDL_TOOL_BINARY}")

# FIXME: don't hardcode elfs. maybe detect from some dir?
set(CDL_ELF_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/tmp_build/hello-goodbye/hello.elf" "${CMAKE_CURRENT_SOURCE_DIR}/tmp_build/hello-goodbye/goodbye.elf")
set(CDL_ELFS "${CMAKE_CURRENT_BINARY_DIR}/hello.elf" "${CMAKE_CURRENT_BINARY_DIR}/goodbye.elf")
add_custom_target(copy_elfs DEPENDS ${CDL_ELFS})
add_custom_command(OUTPUT ${CDL_ELFS}
                   COMMAND ${CMAKE_COMMAND} -E copy ${CDL_ELF_SOURCES} "${CMAKE_CURRENT_BINARY_DIR}"
                   DEPENDS ${CDL_ELF_SOURCES})

BuildCapDLApplication(C_SPEC capdl_spec.c
ELF ${CDL_ELFS}
DEPENDS capdl_c_spec_target
OUTPUT capdl-loader)

include(rootserver)
DeclareRootserver(capdl-loader)
