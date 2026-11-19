// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IERDomain extends com.change_vision.jude.api.inf.model.INamedElement {
    public abstract void setLengthPrecision(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setNotNull(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setDefaultValue(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract java.lang.String getName();
}
