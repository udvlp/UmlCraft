// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface INamedElement extends com.change_vision.jude.api.inf.model.IElement {
    public abstract java.lang.String getName();
    public abstract void setName(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setDefinition(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setVisibility(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
