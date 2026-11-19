// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IERDiagram extends com.change_vision.jude.api.inf.model.IDiagram {
    public abstract void setNotation(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setModelType(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setInitialDisplayLevel(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
