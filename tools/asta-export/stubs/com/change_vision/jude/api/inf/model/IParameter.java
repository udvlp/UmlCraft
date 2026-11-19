// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IParameter extends com.change_vision.jude.api.inf.model.INamedElement {
    public abstract void setDirection(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
