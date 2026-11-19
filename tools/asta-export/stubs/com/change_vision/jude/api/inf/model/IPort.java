// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IPort extends com.change_vision.jude.api.inf.model.IAttribute {
    public abstract void setIsBehavior(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setType(com.change_vision.jude.api.inf.model.IClass p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setTypeExpression(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
