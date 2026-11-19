// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IERSchema extends com.change_vision.jude.api.inf.model.IERPackage {
    public abstract com.change_vision.jude.api.inf.model.IERDatatype[] getDatatypes();
    public abstract java.lang.String getName();
}
