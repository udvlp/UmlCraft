// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.editor;

public class TransactionManager {
    public static void beginTransaction() throws com.change_vision.jude.api.inf.exception.BadTransactionException {
        throw new UnsupportedOperationException();
    }
    public static void endTransaction() throws com.change_vision.jude.api.inf.exception.BadTransactionException {
        throw new UnsupportedOperationException();
    }
    public static void abortTransaction() throws com.change_vision.jude.api.inf.exception.BadTransactionException {
        throw new UnsupportedOperationException();
    }
}
