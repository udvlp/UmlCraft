// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.exception;

public class InvalidEditingException extends java.lang.Exception {
    public static final java.lang.String NAME_DOUBLE_ERROR_KEY = "NAME_DOUBLE_ERROR_KEY";
    public static final java.lang.String NO_NAME_ERROR_KEY = "NO_NAME_ERROR_KEY";
    public java.lang.String getKey() {
        throw new UnsupportedOperationException();
    }
}
