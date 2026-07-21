#!/usr/bin/env bash

onnxruntime_android_version() {
  printf '%s\n' "1.23.2"
}

onnxruntime_android_aar_url_for_version() {
  case "$1" in
    1.23.2)
      printf '%s\n' "https://repo.maven.apache.org/maven2/com/microsoft/onnxruntime/onnxruntime-android/1.23.2/onnxruntime-android-1.23.2.aar"
      ;;
    *)
      echo "No pinned Android ONNX Runtime AAR URL for version '$1'." >&2
      return 1
      ;;
  esac
}

onnxruntime_android_aar_sha256_for_version() {
  case "$1" in
    1.23.2)
      printf '%s\n' "82048d1f462218adae4ba76477089ab0ba76093d84f733540066db1a8ba6b827"
      ;;
    *)
      echo "No pinned Android ONNX Runtime AAR SHA-256 for version '$1'." >&2
      return 1
      ;;
  esac
}

onnxruntime_android_lib_sha256_for_version() {
  case "$1:$2" in
    1.23.2:arm64-v8a)
      printf '%s\n' "e40f09d07dc53726b8bfbf48a7907673b8f86718a057655a62790a39874a7302"
      ;;
    1.23.2:armeabi-v7a)
      printf '%s\n' "57048b8d54896d16355ee367bfc129c5925468ae503b681b8d0cd49ceefa468e"
      ;;
    1.23.2:x86)
      printf '%s\n' "213d91ebb0cfd511c18c0057c69145de0abc6bdc9c63429bf04dcdeaf3fd861a"
      ;;
    1.23.2:x86_64)
      printf '%s\n' "972c17c056eaae946a415d9efdd8018b729639974df075e495f0092441478fb7"
      ;;
    *)
      echo "No pinned Android ONNX Runtime library SHA-256 for version '$1' and ABI '$2'." >&2
      return 1
      ;;
  esac
}

onnxruntime_android_abis() {
  printf '%s\n' \
    "arm64-v8a" \
    "armeabi-v7a" \
    "x86" \
    "x86_64"
}
