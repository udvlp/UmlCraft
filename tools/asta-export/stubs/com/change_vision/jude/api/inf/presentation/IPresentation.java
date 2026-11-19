// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.presentation;

public interface IPresentation {
    public abstract com.change_vision.jude.api.inf.model.IElement getModel();
    public abstract java.lang.String getType();
    public abstract void setProperty(java.lang.String p0, java.lang.String p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
