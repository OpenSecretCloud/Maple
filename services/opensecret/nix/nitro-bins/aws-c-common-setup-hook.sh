addAwsCCommonModuleDir() {
    prependToVar cmakeFlags "-DCMAKE_MODULE_PATH=@out@/lib/cmake/aws-c-common/modules"
}

postHooks+=(addAwsCCommonModuleDir)
