// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IObjectNode extends com.change_vision.jude.api.inf.model.INamedElement {
    public abstract void setInState(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setOrdering(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setUpperBound(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setControl(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setEffect(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setException(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setStream(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
